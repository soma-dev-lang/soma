#!/usr/bin/env python3
"""The harness of IMPROVEMENT_PROGRAM.md: gates that must hold, metrics to move.

    python3 tools/harness/run.py                 # quick: ~1 min, after every change
    python3 tools/harness/run.py --full          # everything CI runs, plus the gates above
    python3 tools/harness/run.py --orbite        # also: the 69,000-line case study (minutes)
    python3 tools/harness/run.py --ref PATH      # also: verdicts against a reference soma binary
    python3 tools/harness/run.py --tighten       # record improved metrics in baseline.json

GATES are yes/no; one red gate is a failed run (exit 1), whatever else improved.
METRICS are numbers compared with tools/harness/baseline.json; one that got
WORSE fails the run too. `--tighten` rewrites the baseline only when every
gate is green and no metric regressed, and only ever towards better: the
baseline is a ratchet, never loosened by hand.

Writes tools/harness/last.json (not tracked). Standard library only.
"""
import argparse, json, os, subprocess, sys, time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
BASELINE = os.path.join(HERE, "baseline.json")
ACCEPTED = os.path.join(HERE, "accepted_verdict_changes.txt")
GATE_LIMIT = 1800      # seconds: a gate that runs longer is red ("timed out"), as it is in CI
ORBITE = os.path.join(ROOT, "site", "cases", "orbite", "src")
ORBITE_LIMIT = 1200
SECONDS_NOISE = 1.30   # a time is worse only past +30 % (and +0.15 s): machines differ, runs jitter
EXPONENT_NOISE = 0.25


def sh(cmd, env=None, cwd=ROOT, timeout=GATE_LIMIT):
    t = time.perf_counter()
    try:
        p = subprocess.run(cmd, capture_output=True, text=True, cwd=cwd, env={**os.environ, **(env or {})}, timeout=timeout)
    except subprocess.TimeoutExpired:
        return 124, f"timed out after {timeout} s", time.perf_counter() - t
    return p.returncode, p.stdout + p.stderr, time.perf_counter() - t


def tool_json(args):
    code, out, _ = sh([sys.executable] + args)
    try:
        return code, json.loads(out[out.index("{"):])
    except ValueError:
        return code, {"error": out[-600:]}


def gate(results, name, ok, detail):
    results["gates"][name] = {"ok": bool(ok), "detail": detail}
    print(f"  {'PASS' if ok else 'FAIL'}  {name:<26} {detail}", flush=True)


def verdicts(soma, ref):
    """Exit codes of check and verify on every tracked program: new binary against the reference."""
    files = [f for f in subprocess.run(["git", "ls-files", "*.cell"], cwd=ROOT, capture_output=True, text=True).stdout.split()
             if "/tests/fixtures/skeleton_review/" not in f]
    accepted = set()
    if os.path.exists(ACCEPTED):
        accepted = {l.split("#")[0].strip() for l in open(ACCEPTED) if l.split("#")[0].strip()}
    import concurrent.futures as cf

    def one(f):
        out = []
        for cmd in ("check", "verify"):
            a = subprocess.run([ref, cmd, f], capture_output=True, cwd=ROOT).returncode
            b = subprocess.run([soma, cmd, f], capture_output=True, cwd=ROOT).returncode
            if a != b:
                out.append(f"{cmd} {f}: {a} -> {b}")
        return f, out
    changed = []
    with cf.ThreadPoolExecutor(8) as pool:
        for f, out in pool.map(one, files):
            if f not in accepted:
                changed += out
    return len(files), changed


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--soma", default=os.environ.get("SOMA", os.path.join(ROOT, "compiler/target/release/soma")))
    ap.add_argument("--ref", help="a reference soma binary (the base of the work): verdict stability")
    ap.add_argument("--full", action="store_true")
    ap.add_argument("--orbite", action="store_true", help="time check / verify on site/cases/orbite (minutes)")
    ap.add_argument("--tighten", action="store_true")
    a = ap.parse_args()
    soma = os.path.abspath(a.soma)
    if not os.path.exists(soma):
        raise SystemExit(f"no binary at {soma} — build it: (cd compiler && cargo build --release)")
    env = {"SOMA": soma}
    res = {"soma": sh([soma, "--version"])[1].strip(), "full": a.full, "gates": {}, "metrics": {}}
    print(f"harness: {res['soma']} ({soma})\n\nGATES")

    # ── gates ──────────────────────────────────────────────────────
    code, fz = tool_json([os.path.join(HERE, "prove_fuzz.py"), "--soma", soma, "--seeds", "2-41", "--bodies", "40"])
    gate(res, "no_false_proof", fz.get("unsound") == 0, fz.get("error") or f"{fz['unsound']} false proof(s) on {fz['bodies']} random handlers")
    code, order = tool_json([os.path.join(HERE, "native_order.py"), "--soma", soma])
    gate(res, "native_argument_order", order.get("differing") == 0,
         order.get("error") or f"{order['differing']} of {order['shapes']} shapes differ" + "".join(f" · {c['shape']}" for c in order["cases"][:6]))
    code, out, _ = sh([soma, "test", os.path.join(HERE, "outliers.cell")])
    gate(res, "outliers", code == 0, out.strip().splitlines()[-1] if out.strip() else "no output")
    code, out, _ = sh(["bash", "tests/run_all.sh"], env)
    gate(res, "cli_fixtures", code == 0, " ".join(l.strip() for l in out.splitlines() if "Passed" in l or "Failed" in l))
    code, out, _ = sh([sys.executable, "tools/check_numeric_oracle.py"], env)
    gate(res, "numeric_oracle", code == 0, out.strip().splitlines()[-1][:90] if out.strip() else "")
    if a.ref:
        n, changed = verdicts(soma, os.path.abspath(a.ref))
        gate(res, "verdicts_unchanged", not changed, f"{len(changed)} verdict(s) changed on {n} tracked programs" + "".join(f" · {c}" for c in changed[:4]))
        # the words too: a count of explored states, a witness run, the order of the lines
        code, same = tool_json([os.path.join(HERE, "verify_same.py"), "--soma", soma, "--ref", os.path.abspath(a.ref)])
        bad = same.get("different", []) + same.get("timeout", [])
        gate(res, "verify_output_unchanged", code == 0 and "error" not in same,
             same.get("error", f"{same.get('same', 0)} of {same.get('files', 0)} programs print what the reference prints, {same.get('ref_timeout', 0)} the reference cannot finish") + "".join(f" · {c}" for c in bad[:4]))
    if a.full:
        code, out, _ = sh(["cargo", "test", "--release", "--no-fail-fast"], cwd=os.path.join(ROOT, "compiler"))
        passed = sum(int(l.split()[3]) for l in out.splitlines() if l.startswith("test result"))
        failed = [l.split()[1] for l in out.splitlines() if l.startswith("test ") and l.rstrip().endswith("FAILED")]
        gate(res, "cargo_test", code == 0, f"{passed} passed, {len(failed)} failed" + "".join(f" · {f}" for f in failed[:5]))
        res["metrics"]["rust_tests"] = passed
        code, out, _ = sh([sys.executable, "tools/diff_fuzz.py", "--files", "10", "--seed", "1"], env)
        gate(res, "native_values", code == 0, out.strip().splitlines()[-1][:110] if out.strip() else "")
        for tool in ("split_roundtrip", "frame_oracle", "skeleton_monotone"):
            code, out, _ = sh([sys.executable, f"tools/{tool}.py", "--soma", soma])
            gate(res, tool, code == 0, out.strip().splitlines()[-1][:110] if out.strip() else "")
        code, out, _ = sh([sys.executable, "tools/gen_grammar.py", "--check"])
        gate(res, "grammar_artifacts", code == 0, (out.strip().splitlines() or ["up to date"])[-1][:110])

    # ── metrics ────────────────────────────────────────────────────
    m = res["metrics"]
    if "false_fail" in fz:
        m["prover_false_failures"] = fz["false_fail"]       # lower is better
        m["prover_true_clauses_proven"] = fz["proven"]      # higher is better
    _, sc = tool_json([os.path.join(HERE, "scale.py"), "--soma", soma])
    for name, p in sc.get("probes", {}).items():
        m[f"scale.{name}.seconds"] = p["seconds"] if p["seconds"] is not None else 9999.0
        if p["exponent"] is not None:
            m[f"scale.{name}.exponent"] = p["exponent"]

    if a.orbite:
        # the large application Soma has: 13,657 lines of skeleton, 59 files, 97 rules between machines
        for name, cmd in (("check_app", ["check", "app.cell"]), ("check_prod", ["check", "prod.cell"]),
                          ("verify_app", ["verify", "app.cell"]), ("verify_prod", ["verify", "prod.cell"])):
            code, out, took = sh([soma] + cmd, cwd=ORBITE, timeout=ORBITE_LIMIT)
            m[f"orbite.{name}.seconds"] = 9999.0 if code == 124 else round(took, 1)
            if code not in (0, 124):
                gate(res, f"orbite_{name}", False, out.strip().splitlines()[-1][:110] if out.strip() else f"exit {code}")

    base = json.load(open(BASELINE)) if os.path.exists(BASELINE) else {}
    print("\nMETRICS" + " " * 37 + "baseline      now")
    regressed, improved = [], {}
    for k in sorted(m):
        v, b = m[k], base.get(k)
        higher = k in ("prover_true_clauses_proven", "rust_tests")
        mark = ""
        if b is not None:
            if higher:
                worse, better = v < b, v > b
            elif k.endswith(".seconds"):
                worse, better = v > b * SECONDS_NOISE and v > b + 0.15, v < b / SECONDS_NOISE
            elif k.endswith(".exponent"):
                worse, better = v > b + EXPONENT_NOISE, v < b - EXPONENT_NOISE
            else:
                worse, better = v > b, v < b
            mark = "WORSE" if worse else ("better" if better else "")
            if worse:
                regressed.append(k)
            if better:
                improved[k] = v
        else:
            improved[k] = v
        print(f"  {k:<42} {'' if b is None else b:>8} {v:>8}  {mark}")
    res["regressed"], res["improved"] = regressed, sorted(improved)
    json.dump(res, open(os.path.join(HERE, "last.json"), "w"), indent=1)

    red = [g for g, r in res["gates"].items() if not r["ok"]]
    print()
    if red:
        print(f"FAILED — red gate(s): {', '.join(red)}")
    if regressed:
        print(f"FAILED — metric(s) worse than the baseline: {', '.join(regressed)}")
    if not red and not regressed:
        print(f"OK — {len(res['gates'])} gates green, {len(improved)} metric(s) better than the baseline")
        if a.tighten and improved:
            base.update(improved)
            json.dump(dict(sorted(base.items())), open(BASELINE, "w"), indent=1)
            print(f"baseline.json tightened: {', '.join(sorted(improved))}")
    sys.exit(1 if red or regressed else 0)


if __name__ == "__main__":
    main()
