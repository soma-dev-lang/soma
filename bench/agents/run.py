#!/usr/bin/env python3
"""Agent-convergence benchmark (spec A7): how many rounds does a model need
to turn a task into a Soma program that passes `check` and its tests, and
how much of what the program promises ends up PROVEN?

    python3 bench/agents/run.py --agent reference            # harness self-test: the corpus solutions
    python3 bench/agents/run.py --agent-cmd 'claude -p' --tasks 40 --rounds 5 --out results/claude.json
    python3 bench/agents/run.py --report results/*.json      # the comparison table

An agent is a command that reads the prompt on stdin and prints the program
(a ```soma fence around it is fine). It gets, each round: the task
(scenario, contracts, acceptance tests) and, after a failed round, its last
program with the diagnostics `soma check --json` / `soma test --json` gave.
It never gets the reference solution. The language guide it is given is the
one the binary ships (`soma docs agent`), so results are comparable across
releases.

Per task: check passed on the first answer, rounds to green (or failure),
characters read and written (a proxy for tokens when the agent does not
report them), and — once green — the verdict classes of `soma verify
--attest`: proven / runtime-checked / unproven.
"""
import argparse
import concurrent.futures as cf
import json
import os
import random
import re
import shutil
import subprocess
import sys
import tempfile
import time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
SOMA = os.environ.get("SOMA", os.path.join(ROOT, "compiler/target/release/soma"))


def soma(args, cwd, timeout=120):
    try:
        r = subprocess.run([SOMA, *args], capture_output=True, text=True, timeout=timeout, cwd=cwd)
        return r.returncode, r.stdout, r.stderr
    except subprocess.TimeoutExpired:
        return -9, "", "timeout"


def guide():
    return soma(["docs", "agent"], ROOT)[1]


def first_prompt(task, guide_text):
    return f"""You are writing a program in Soma. The language guide follows; then the task.

<guide>
{guide_text}
</guide>

<task>
{task['scenario']}

Implement these contracts (cells, signals, memory slots) exactly:

{task['contracts']}

The program must pass these acceptance tests (do NOT include them or any
other `cell test` in your answer — they are appended to your file):

{task['tests']}
</task>

Answer with the complete .cell source and nothing else."""


def retry_prompt(task, program, stage, diagnostics):
    return f"""Your program for this task does not pass `soma {stage}` yet.

<task>
{task['scenario']}

Contracts:

{task['contracts']}

Acceptance tests (appended to your file; do not include them):

{task['tests']}
</task>

<your_program>
{program}
</your_program>

<diagnostics>
{diagnostics}
</diagnostics>

Answer with the complete corrected .cell source and nothing else."""


def extract_program(text):
    fences = re.findall(r"```(?:soma|cell)?\n(.*?)```", text, re.S)
    if fences:
        text = max(fences, key=len)
    # the acceptance tests are appended by the harness
    return re.sub(r"(?ms)^cell test \w+\s*\{.*?^\}\n?", "", text).strip() + "\n"


def ask(agent, prompt, task, cwd):
    if agent == "reference":
        return task["reference"]
    if agent == "empty":
        return "cell Nothing {\n    on noop() { return 1 }\n}\n"
    r = subprocess.run(agent, shell=True, input=prompt, capture_output=True, text=True, cwd=cwd, timeout=900)
    return r.stdout


def diagnostics_of(stage, out):
    try:
        doc = json.loads(out)
    except Exception:
        return out[-4000:]
    if stage == "check":
        keep = [{k: e.get(k) for k in ("code", "message", "line", "col", "source_line", "fix")} for e in doc.get("errors", [])]
    else:
        keep = [r for r in doc.get("results", []) if r.get("status") != "pass"] or doc.get("errors") or doc.get("error")
    return json.dumps(keep, indent=1, ensure_ascii=False)[:8000]


def run_task(task, agent, rounds, guide_text, keep):
    d = tempfile.mkdtemp(prefix="soma_bench_")
    if task.get("soma_toml"):
        open(os.path.join(d, "soma.toml"), "w").write(task["soma_toml"])
    res = {"id": task["id"], "domain": task["domain"], "green": False, "rounds": None, "check_first_pass": False,
           "chars_in": 0, "chars_out": 0, "stages": [], "seconds": 0.0}
    t0 = time.time()
    prompt = first_prompt(task, guide_text)
    program = ""
    try:
        for r in range(1, rounds + 1):
            res["chars_in"] += len(prompt)
            answer = ask(agent, prompt, task, d)
            res["chars_out"] += len(answer)
            program = extract_program(answer)
            open(os.path.join(d, "app.cell"), "w").write(program + "\n" + task["tests"] + "\n")
            code, out, err = soma(["check", "--json", "app.cell"], d)
            if code != 0:
                res["stages"].append("check")
                prompt = retry_prompt(task, program, "check", diagnostics_of("check", out or err))
                continue
            if r == 1:
                res["check_first_pass"] = True
            code, out, err = soma(["test", "--json", "app.cell"], d)
            if code != 0:
                res["stages"].append("test")
                prompt = retry_prompt(task, program, "test", diagnostics_of("test", out or err))
                continue
            res["green"], res["rounds"] = True, r
            res["stages"].append("green")
            soma(["verify", "--attest", "attest.json", "app.cell"], d)
            try:
                res["verdicts"] = json.load(open(os.path.join(d, "attest.json")))["summary"]
            except Exception:
                res["verdicts"] = {}
            break
    except subprocess.TimeoutExpired:
        res["stages"].append("timeout")
    res["seconds"] = round(time.time() - t0, 2)
    if keep:
        res["dir"] = d
    else:
        shutil.rmtree(d, ignore_errors=True)
    return res


def summarize(results, rounds):
    n = len(results)
    green = [r for r in results if r["green"]]
    v = {k: sum(r.get("verdicts", {}).get(k, 0) for r in green) for k in ("proven", "runtime-checked", "unproven")}
    stated = sum(v.values())
    return {
        "tasks": n,
        "green": len(green),
        "green_rate": round(len(green) / n, 3) if n else 0,
        "check_first_pass_rate": round(sum(r["check_first_pass"] for r in results) / n, 3) if n else 0,
        "green_first_round_rate": round(sum(1 for r in green if r["rounds"] == 1) / n, 3) if n else 0,
        "mean_rounds_to_green": round(sum(r["rounds"] for r in green) / len(green), 2) if green else None,
        "max_rounds": rounds,
        "mean_chars_to_green": round(sum(r["chars_in"] + r["chars_out"] for r in green) / len(green)) if green else None,
        "proven_fraction": round(v["proven"] / stated, 3) if stated else None,
        "verdicts": v,
    }


def report(paths):
    rows = []
    for p in paths:
        doc = json.load(open(p))
        s = doc["summary"]
        rows.append((doc.get("agent", p), doc.get("soma", "?"), s))
    cols = ["agent", "soma", "tasks", "green", "check@1", "green@1", "rounds", "chars", "proven"]
    print(" | ".join(cols))
    print(" | ".join("---" for _ in cols))
    for agent, ver, s in rows:
        print(" | ".join(str(x) for x in [agent, ver, s["tasks"], f"{s['green_rate']:.0%}", f"{s['check_first_pass_rate']:.0%}",
              f"{s['green_first_round_rate']:.0%}", s["mean_rounds_to_green"], s["mean_chars_to_green"],
              "—" if s["proven_fraction"] is None else f"{s['proven_fraction']:.0%}"]))


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--agent", choices=["reference", "empty"], help="a built-in agent (harness self-test)")
    ap.add_argument("--agent-cmd", help="a command: prompt on stdin, program on stdout")
    ap.add_argument("--name", help="how the agent is named in the results")
    ap.add_argument("--tasks", type=int, default=0, help="how many tasks (0: all), sampled with --seed across domains")
    ap.add_argument("--only", help="a task id or a domain")
    ap.add_argument("--rounds", type=int, default=5)
    ap.add_argument("--jobs", type=int, default=4)
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--out")
    ap.add_argument("--keep", action="store_true", help="keep each task's directory")
    ap.add_argument("--report", nargs="+", help="print the comparison table of result files")
    a = ap.parse_args()
    if a.report:
        return report(a.report)
    agent = a.agent or a.agent_cmd
    if not agent:
        ap.error("--agent or --agent-cmd")
    tasks_path = os.path.join(HERE, "tasks.json")
    if not os.path.exists(tasks_path):
        subprocess.run([sys.executable, os.path.join(HERE, "make_tasks.py")], check=True)
    tasks = json.load(open(tasks_path))
    if a.only:
        tasks = [t for t in tasks if t["id"] == a.only or t["domain"] == a.only]
    if a.tasks and a.tasks < len(tasks):
        random.Random(a.seed).shuffle(tasks)
        tasks = sorted(tasks[:a.tasks], key=lambda t: t["id"])
    guide_text = guide()
    with cf.ThreadPoolExecutor(a.jobs) as ex:
        results = list(ex.map(lambda t: run_task(t, agent, a.rounds, guide_text, a.keep), tasks))
    doc = {
        "agent": a.name or agent,
        "soma": soma(["--version"], ROOT)[1].strip(),
        "rounds": a.rounds,
        "seed": a.seed,
        "summary": summarize(results, a.rounds),
        "results": results,
    }
    if a.out:
        os.makedirs(os.path.dirname(os.path.abspath(a.out)), exist_ok=True)
        json.dump(doc, open(a.out, "w"), indent=1)
    print(json.dumps(doc["summary"], indent=1))
    failed = [r["id"] for r in results if not r["green"]]
    if failed:
        print(f"not green ({len(failed)}): " + ", ".join(failed[:20]) + (" …" if len(failed) > 20 else ""), file=sys.stderr)


if __name__ == "__main__":
    main()
