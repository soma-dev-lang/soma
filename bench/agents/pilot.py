#!/usr/bin/env python3
"""Skeleton-first pilot: does an agent build a large program better when the
structure is written, proven and frozen before any body?

Three arms on one application, same model, same token ceiling:

  cells     plain cells: one agent writes the whole program from the spec
  skeleton  skeleton first, one agent: it designs the skeletons (checked,
            verified, frozen), then writes all the flesh
  holes     skeleton first, one sub-agent per hole: the same design step,
            then each hole is filled by a fresh agent that gets the BRIEF of
            that signal and nothing else

PRIMARY MEASURE (fixed before any run): the pass rate of the hidden
cross-cell scenario tests (`hidden/scenario.cell`). Secondary: hidden
per-signal tests, structural drift, refused structural writes, tokens
(design counted), rounds.

What the harness guarantees, and `--selftest` checks with agents that cheat:

  1. An agent is a pure function prompt -> text. It has no tool, no file
     system, no shell: it cannot read the hidden tests or run `soma`.
  2. After the freeze the harness alone writes files, and only flesh:
     an answer that holds anything but `flesh` blocks (arm `skeleton`) or
     `on` handlers of the hole and its `_helpers` (arm `holes`) is refused
     and counted. `--amend` is never run; the frozen file and its lock are
     hashed, and any difference aborts the arm.
  3. The hidden tests are never in a prompt: every prompt is recorded and
     audited against the hidden files (assert lines, and every string
     literal the spec does not also contain).
  4. Every call — design included — is charged to the arm's ceiling before
     the next one is made.

    python3 bench/agents/pilot.py --selftest
    python3 bench/agents/pilot.py --app marketplace --arm all --model sonnet --cap 1500000 --out bench/agents/results/pilot.json
    python3 bench/agents/pilot.py --report bench/agents/results/pilot.json
"""
import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
SOMA = os.environ.get("SOMA", os.path.join(ROOT, "compiler/target/release/soma"))
PRIMARY = "scenario"   # the hidden test file whose pass rate is the primary measure


def soma(args, cwd, timeout=180):
    try:
        r = subprocess.run([SOMA, *args], capture_output=True, text=True, timeout=timeout, cwd=cwd,
                           env=dict(os.environ, NO_COLOR="1", SOMA_LLM_MOCK="fixed:ok"))
        return r.returncode, r.stdout + r.stderr
    except subprocess.TimeoutExpired:
        return -9, "timeout"


def sha(path):
    return hashlib.sha256(open(path, "rb").read()).hexdigest()


# ── agents ───────────────────────────────────────────────────────────

class CapReached(Exception):
    pass


class Tampered(Exception):
    pass


class Meter:
    """Every call of an arm, charged to one ceiling."""

    def __init__(self, cap):
        self.cap, self.calls, self.prompts = cap, [], []

    @property
    def total(self):
        return sum(c["tokens"] for c in self.calls)

    def phase(self, name):
        return sum(c["tokens"] for c in self.calls if c["phase"] == name)

    def charge(self, phase, tokens, system, prompt):
        self.calls.append({"phase": phase, "tokens": tokens})
        self.prompts.append(system + "\n" + prompt)

    def before_call(self):
        if self.total >= self.cap:
            raise CapReached()


class CliAgent:
    """`claude -p` with no tool and no settings: prompt in, text out, real token counts."""

    def __init__(self, model):
        self.model = model
        self.cwd = tempfile.mkdtemp(prefix="pilot-agent-")   # empty: nothing to read even if it could

    def __call__(self, system, prompt):
        sysfile = os.path.join(self.cwd, "system.txt")
        open(sysfile, "w").write(system)
        cmd = ["claude", "-p", "--output-format", "json", "--tools", "", "--setting-sources", "",
               "--strict-mcp-config", "--no-session-persistence", "--disable-slash-commands",
               "--model", self.model, "--system-prompt-file", sysfile]
        for attempt in range(3):
            r = subprocess.run(cmd, input=prompt, capture_output=True, text=True, cwd=self.cwd, timeout=1800)
            try:
                d = json.loads(r.stdout)
                u = d.get("usage") or {}
                tokens = sum(int(u.get(k) or 0) for k in ("input_tokens", "cache_creation_input_tokens", "cache_read_input_tokens", "output_tokens"))
                if d.get("is_error") or not d.get("result"):
                    raise ValueError(str(d.get("result"))[:200])
                # for the report: how much of it was a cache read, and the cost
                self.last = {"cache_read": int(u.get("cache_read_input_tokens") or 0), "output": int(u.get("output_tokens") or 0), "usd": d.get("total_cost_usd") or 0}
                return d["result"], tokens
            except (ValueError, json.JSONDecodeError) as e:
                err = f"{e} {r.stderr[:200]}"
                time.sleep(5 * (attempt + 1))
        raise RuntimeError(f"agent call failed: {err}")


def approx_tokens(*texts):
    return sum(len(t) for t in texts) // 4


# ── text ─────────────────────────────────────────────────────────────

def code_of(answer):
    blocks = re.findall(r"```(?:soma|cell)?[ \t]*\n(.*?)```", answer, re.S)
    return ("\n\n".join(b.strip("\n") for b in blocks) if blocks else answer.strip()) + "\n"


def top_level_items(src):
    """(first word(s), text) of every top-level item, by brace depth; comments and strings skipped."""
    items, depth, start, i, n = [], 0, None, 0, len(src)
    while i < n:
        c = src[i]
        if src.startswith("//", i):
            j = src.find("\n", i)
            i = n if j < 0 else j
            continue
        if c == '"':
            i += 1
            while i < n and src[i] != '"':
                i += 2 if src[i] == "\\" else 1
            i += 1
            continue
        if depth == 0 and start is None and not c.isspace():
            start = i
        if c == "{":
            depth += 1
        elif c == "}":
            depth -= 1
            if depth == 0 and start is not None:
                items.append(src[start:i + 1])
                start = None
        elif c == "\n" and depth == 0 and start is not None and src[start:i].lstrip().startswith("use "):
            items.append(src[start:i])
            start = None
        i += 1
    if start is not None and src[start:].strip():
        items.append(src[start:])
    return items


def head(item):
    return " ".join(item.split()[:2])


def only_skeletons(src):
    """Design answer: skeletons and sum types, nothing else. -> reason it is refused, or None."""
    items = top_level_items(src)
    if not any(i.startswith("skeleton") for i in items):
        return "the answer holds no `skeleton`"
    for it in items:
        if not (it.startswith("skeleton ") or it.startswith("cell type ")):
            return f"the design step takes `skeleton` (and `cell type`) declarations only — found `{head(it)} …`"
    return None


def only_flesh(src, skeleton_names):
    items = top_level_items(src)
    if not items:
        return "the answer holds no `flesh` block"
    for it in items:
        m = re.match(r"flesh\s+(\w+)\s*\{", it)
        if not m:
            return f"after the freeze only `flesh Name {{ on … }}` blocks are accepted — found `{head(it)} …` (the structure is frozen: it is not yours to change)"
        if m.group(1) not in skeleton_names:
            return f"`flesh {m.group(1)}`: there is no skeleton of that name"
    return None


def only_handlers(src, signal):
    """Hole answer: `on <signal>` and `on _helper` handlers. -> reason, or None."""
    items = top_level_items(src)
    names = []
    for it in items:
        m = re.match(r"on\s+(\w+)\s*\(", it)
        if not m:
            return f"the answer must be `on {signal}(…) {{ … }}` (and `on _helper(…)` handlers) only — found `{head(it)} …`"
        names.append(m.group(1))
    if signal not in names:
        return f"the answer has no `on {signal}(…)`"
    for nm in names:
        if nm != signal and not nm.startswith("_"):
            return f"`on {nm}`: only the hole `{signal}` and private `_helpers` may be written here"
    return None


def errors_of(out, limit=12):
    """The error blocks of a `soma check` report."""
    blocks, cur = [], None
    for line in out.splitlines():
        if line.startswith("error"):
            cur = [line]
            blocks.append(cur)
        elif cur is not None and (line.startswith(" ") or line.startswith("\t")):
            cur.append(line)
        else:
            cur = None
    return "\n".join("\n".join(b[:6]) for b in blocks[:limit]) or out.strip()[-1500:]


# ── the three arms ───────────────────────────────────────────────────

GUIDE = None


def guides():
    global GUIDE
    if GUIDE is None:
        GUIDE = (soma(["docs", "agent"], ROOT)[1], soma(["docs", "skeleton"], ROOT)[1])
    return GUIDE


def system_for(arm, phase="design"):
    agent, skeleton = guides()
    if arm == "holes" and phase == "fill":
        # a hole is filled from its brief and the one-page card, not the whole guide
        card = soma(["docs", "flesh"], ROOT)[1]
        return "You write ONE handler body in Soma from a brief. You have no tools: answer with code in ```soma fences.\n\n<card>\n" + card + "\n</card>\n"
    intro = "You write programs in Soma, a small language described below. You have no tools: answer with code in ```soma fences, and nothing you could not justify from the text you are given.\n\n"
    if arm == "cells":
        return intro + "<guide>\n" + agent + "\n</guide>\n"
    return intro + "<guide>\n" + agent + "\n</guide>\n\n<skeleton-guide>\n" + skeleton + "\n</skeleton-guide>\n"


class Run:
    def __init__(self, app_dir, arm, agent, cap, rounds, work):
        self.app, self.arm, self.agent, self.rounds, self.work = app_dir, arm, agent, rounds, work
        self.meter = Meter(cap)
        self.spec = open(os.path.join(app_dir, "spec.md")).read()
        self.refused, self.round_count, self.frozen = [], 0, None
        self.log = []
        os.makedirs(os.path.join(work, "lib"), exist_ok=True)
        toml = os.path.join(app_dir, "pilot.toml")
        open(os.path.join(work, "soma.toml"), "w").write(open(toml).read() if os.path.exists(toml) else '[package]\nname = "pilot"\nversion = "0.1.0"\nedition = "2027"\n')

    def ask(self, phase, prompt):
        self.meter.before_call()
        system = system_for(self.arm, phase)
        answer, tokens = self.agent(system, prompt)
        if tokens is None:
            tokens = approx_tokens(system, prompt, answer)
        self.meter.charge(phase, tokens, system, prompt)
        self.meter.calls[-1].update(getattr(self.agent, "last", {}))
        self.round_count += 1
        return answer

    def path(self, *p):
        return os.path.join(self.work, *p)

    def write(self, rel, text):
        # the one place the harness writes a program file
        if self.frozen and rel == "app.cell":
            raise Tampered("the harness itself tried to rewrite the frozen skeleton")
        open(self.path(rel), "w").write(text)

    def guard(self):
        """After the freeze: the skeleton file and its lock are what they were."""
        if not self.frozen:
            return
        now = (sha(self.path("app.cell")), sha(self.path("skeleton.lock")))
        if now != self.frozen:
            raise Tampered("the frozen skeleton or its lock changed")

    # arm 1 ───────────────────────────────────────────────────────────
    def cells(self):
        prompt = f"<spec>\n{self.spec}\n</spec>\n\nWrite the complete program as ordinary `cell`s, in one file. No `cell test` (tests are run separately), no `use`. Answer with the whole file."
        program = ""
        for _ in range(self.rounds):
            program = code_of(self.ask("write", prompt))
            self.write("app.cell", program)
            code, out = soma(["check", "app.cell"], self.work)
            self.log.append(f"cells: check {'ok' if code == 0 else 'fails'}")
            if code == 0:
                break
            prompt = f"<spec>\n{self.spec}\n</spec>\n\nYour program does not pass `soma check`:\n\n<program>\n{program}</program>\n\n<diagnostics>\n{errors_of(out)}\n</diagnostics>\n\nAnswer with the whole corrected file."

    # the design step of arms 2 and 3 ─────────────────────────────────
    def design(self):
        ask = (f"<spec>\n{self.spec}\n</spec>\n\n"
               "Write the SKELETONS of this program and nothing else: one `skeleton` per cell of the spec, with its memory, "
               "invariants, state machines, and a face that declares every signal of the spec with its exact signature and its "
               "frame (`reads` / `writes` / `moves` / `calls` / `emits`). No `flesh`, no handler body, no `use`, no test.\n"
               "Each body will be written later from the BRIEF of its signal alone — by someone who sees that signal's comment, "
               "its slots, its machine and the signatures of what it may call, and not this spec. So above every signal write "
               "a `//` comment that says exactly what its body must do and what its answer means (every field of a returned "
               "map, every error kind and when), and comment each slot (what the key is, what the value is).\n"
               "Answer with one file.")
        prompt, skeleton = ask, ""
        for _ in range(self.rounds):
            skeleton = code_of(self.ask("design", prompt))
            why = only_skeletons(skeleton)
            if why is None:
                self.write("app.cell", "use lib::flesh\n\n" + skeleton)
                self.write("lib/flesh.cell", "// the flesh — written by the harness, one block per accepted answer\n")
                code, out = soma(["check", "app.cell"], self.work)
                if code == 0:
                    vcode, vout = soma(["verify", "app.cell", "--summary"], self.work)
                    bad = [l for l in vout.splitlines() if l.strip().startswith("✗")]
                    if vcode == 0 and not bad:
                        self.log.append("design: checks and verifies")
                        break
                    why = "`soma verify` refutes the design:\n" + "\n".join(bad or vout.splitlines()[-8:])
                else:
                    why = "`soma check` refuses it:\n" + errors_of(out)
            self.log.append("design: refused — " + why.splitlines()[0][:120])
            prompt = f"{ask}\n\nYour previous answer was not accepted.\n\n<previous>\n{skeleton}</previous>\n\n<diagnostics>\n{why}\n</diagnostics>\n\nAnswer with the whole corrected file."
        else:
            return False
        code, out = soma(["skeleton", "app.cell", "--freeze"], self.work)
        if code != 0:
            self.log.append("freeze failed: " + out.strip()[-200:])
            return False
        os.chmod(self.path("app.cell"), 0o444)
        os.chmod(self.path("skeleton.lock"), 0o444)
        self.frozen = (sha(self.path("app.cell")), sha(self.path("skeleton.lock")))
        return True

    def skeleton_names(self):
        return re.findall(r"^skeleton\s+(?:agent\s+)?(\w+)", open(self.path("app.cell")).read(), re.M)

    def refuse(self, what, why):
        self.refused.append({"at": what, "why": why})
        self.log.append(f"{what}: REFUSED — {why[:140]}")

    # arm 2 ───────────────────────────────────────────────────────────
    def skeleton(self):
        if not self.design():
            return
        frozen_text = open(self.path("app.cell")).read().split("\n", 2)[2]
        ask = (f"<spec>\n{self.spec}\n</spec>\n\n<skeleton frozen=\"true\">\n{frozen_text}</skeleton>\n\n"
               "The skeletons above are frozen. Write the FLESH: one `flesh Name { on signal(params) { … } … }` per skeleton, "
               "a body for every signal. Only `flesh` blocks; private helpers are `on _name(x: Type)`. A body may only do what "
               "its signal's frame says. Answer with all the flesh blocks.")
        prompt = ask
        for _ in range(self.rounds):
            flesh = code_of(self.ask("fill", prompt))
            self.guard()
            why = only_flesh(flesh, self.skeleton_names())
            if why is not None:
                self.refuse("flesh", why)
            else:
                self.write("lib/flesh.cell", flesh)
                code, out = soma(["check", "app.cell"], self.work)
                self.guard()
                if code == 0:
                    self.log.append("flesh: check ok")
                    return
                why = "`soma check` refuses it:\n" + errors_of(out)
                self.log.append("flesh: check fails")
            prompt = f"{ask}\n\nYour previous answer was not accepted.\n\n<previous>\n{flesh}</previous>\n\n<diagnostics>\n{why}\n</diagnostics>\n\nAnswer with all the flesh blocks, corrected."
        # nothing accepted: every signal stays a hole
        if soma(["check", "app.cell"], self.work)[0] != 0:
            self.write("lib/flesh.cell", "// no flesh passed `soma check`\n")

    # arm 3 ───────────────────────────────────────────────────────────
    def holes(self):
        if not self.design():
            return
        code, out = soma(["holes", "app.cell", "--json"], self.work)
        order = [h["signal"] for h in json.loads(out)["holes"]]
        accepted = open(self.path("lib/flesh.cell")).read()
        for name in order:
            cell, signal = name.split(".", 1)
            brief = soma(["skeleton", "app.cell", name], self.work)[1]
            ask = (f"Fill one hole of a Soma program. This brief is everything you get — the comment of the signal says what "
                   f"the body must do; the body may only touch what the brief lists.\n\n<brief>\n{brief}</brief>\n\n"
                   f"Answer with `on {signal}(…) {{ … }}` (parameters without types, as on the brief's last line) and, if you "
                   f"need them, private helpers `on _name(x: Type) {{ … }}`. Nothing else.")
            prompt = ask
            try:
                for _ in range(self.rounds):
                    body = code_of(self.ask("fill", prompt))
                    self.guard()
                    why = only_handlers(body, signal)
                    if why is not None:
                        self.refuse(name, why)
                    else:
                        block = f"\nflesh {cell} {{\n" + "\n".join("    " + l if l.strip() else l for l in body.strip("\n").splitlines()) + "\n}\n"
                        self.write("lib/flesh.cell", accepted + block)
                        code, out = soma(["check", "app.cell"], self.work)
                        self.guard()
                        if code == 0:
                            accepted += block
                            self.log.append(f"{name}: filled")
                            break
                        self.write("lib/flesh.cell", accepted)      # not accepted: it stays a hole
                        why = "`soma check` refuses it:\n" + errors_of(out)
                        self.log.append(f"{name}: check fails — " + (errors_of(out).splitlines() or [""])[0][:220])
                    prompt = f"{ask}\n\nYour previous answer was not accepted.\n\n<previous>\n{body}</previous>\n\n<diagnostics>\n{why}\n</diagnostics>\n\nAnswer again, corrected."
            except CapReached:
                self.write("lib/flesh.cell", accepted)
                raise

    # ───────────────────────────────────────────────────────────────
    def run(self):
        status = "done"
        try:
            getattr(self, self.arm)()
        except CapReached:
            status = "cap reached"
        except Tampered as e:
            status = f"ABORTED: {e}"
        try:
            self.guard()
        except Tampered as e:
            status = f"ABORTED: {e}"
        return self.result(status)

    def result(self, status):
        res = {"arm": self.arm, "status": status, "rounds": self.round_count, "log": self.log,
               "tokens": {"total": self.meter.total, "design": self.meter.phase("design"), "fill": self.meter.phase("fill") + self.meter.phase("write"), "cap": self.meter.cap},
               "tokens_detail": {k: round(sum(c.get(k, 0) for c in self.meter.calls), 4) for k in ("cache_read", "output", "usd")},
               "refused_structural_writes": self.refused,
               "amend_runs": 0,   # the harness has no code path that runs it
               "structure_drift": None if not self.frozen else (sha(self.path("app.cell")), sha(self.path("skeleton.lock"))) != self.frozen}
        code, out = soma(["check", "app.cell"], self.work) if os.path.exists(self.path("app.cell")) else (1, "no program")
        res["check"] = code == 0
        holes = re.search(r"○ (\d+) hole", out)
        res["holes_left"] = int(holes.group(1)) if holes else 0
        if code == 0:
            v = soma(["verify", "app.cell", "--summary"], self.work)[1].strip().splitlines()
            res["verify"] = " | ".join(v[-2:])[-240:]
        res["hidden"] = score(self.app, self.work, code == 0)
        res["leaks"] = audit(self.app, self.meter.prompts, self.spec)
        return res


# ── hidden tests ─────────────────────────────────────────────────────

def count_tests(src):
    return len(re.findall(r"^\s*(?:assert|assert_fails)\b", src, re.M))


def score(app_dir, work, checks):
    """Pass counts of each hidden test file, on a copy of the program."""
    out = {}
    for name in sorted(os.listdir(os.path.join(app_dir, "hidden"))):
        if not name.endswith(".cell"):
            continue
        tests = open(os.path.join(app_dir, "hidden", name)).read()
        total, passed = count_tests(tests), 0
        if checks:
            tmp = tempfile.mkdtemp(prefix="pilot-score-")
            shutil.copytree(work, os.path.join(tmp, "p"), ignore=shutil.ignore_patterns(".soma_data", ".soma_env"))
            app = os.path.join(tmp, "p", "app.cell")
            os.chmod(app, 0o644)
            open(app, "a").write("\n" + tests)
            _, report = soma(["test", "app.cell"], os.path.join(tmp, "p"))
            m = re.findall(r"(\d+) tests?: (\d+) passed, (\d+) failed", report)
            if m:
                passed = min(int(m[-1][1]), total)
            shutil.rmtree(tmp, ignore_errors=True)
        out[name[:-5]] = {"passed": passed, "total": total}
    return out


def audit(app_dir, prompts, spec):
    """Hidden material found in what agents were sent (or in the spec)."""
    hidden = "".join(open(os.path.join(app_dir, "hidden", f)).read() for f in sorted(os.listdir(os.path.join(app_dir, "hidden"))) if f.endswith(".cell"))
    norm = lambda s: " ".join(s.split())
    lines = {norm(l) for l in hidden.splitlines() if l.strip().startswith(("assert", "mock"))}
    # test data, not vocabulary: an id-like literal ("o7", "u-pen", "201") or a long one —
    # a word such as "single" is in any design of the same spec
    literals = {m for m in re.findall(r'"((?:[^"\\]|\\.)*)"', hidden)
                if m not in spec and len(m) >= 2 and (re.search(r"[0-9@-]", m) or len(m) >= 12) and not m.startswith(("/", "{"))}
    # the language guide is in every prompt: what it happens to contain is not a leak
    guide_text = "".join(guides())
    literals = {m for m in literals if f'"{m}"' not in guide_text}
    sent = norm("\n".join(prompts))
    found = [f"assert line in a prompt: {l}" for l in sorted(lines) if l in sent]
    found += [f"assert line in the spec: {l}" for l in sorted(lines) if l in norm(spec)]
    found += [f"hidden-only literal in a prompt: {lit!r}" for lit in sorted(literals) if f'"{lit}"' in sent]
    found += ["a prompt names the hidden directory" for p in prompts if "/hidden/" in p][:1]
    return found


# ── fake agents, for the self-test ───────────────────────────────────

def reference_agent(app_dir):
    ref = os.path.join(app_dir, "reference")
    app = open(os.path.join(ref, "app.cell")).read()
    skeletons = "\n".join(i for i in top_level_items(app) if i.startswith("skeleton"))
    flesh = "\n".join(open(os.path.join(ref, "lib", f)).read() for f in sorted(os.listdir(os.path.join(ref, "lib"))))
    handlers = {}
    for block in top_level_items(flesh):
        m = re.match(r"flesh\s+(\w+)\s*\{(.*)\}\s*$", block, re.S)
        if m:
            for h in top_level_items(m.group(2)):
                handlers[(m.group(1), re.match(r"on\s+(\w+)", h).group(1))] = h

    def agent(system, prompt):
        if "Fill one hole" in prompt:
            sig = re.search(r"Answer with `on (\w+)\(", prompt).group(1)
            cell = re.search(r"flesh (\w+) \{ on " + sig, prompt).group(1)
            helpers = [h for (c, n), h in handlers.items() if c == cell and n.startswith("_") and n + "(" in handlers[(cell, sig)]]
            return "```soma\n" + "\n".join([handlers[(cell, sig)]] + helpers) + "\n```", None
        if "Write the FLESH" in prompt:
            return "```soma\n" + flesh + "\n```", None
        if "Write the SKELETONS" in prompt:
            return "```soma\n" + skeletons + "\n```", None
        # arm `cells`: the same program in one file
        return "```soma\n" + skeletons + "\n" + flesh + "\n```", None
    return agent, skeletons, flesh, handlers


def selftest(app_dir):
    ok = True

    def check(label, cond, detail=""):
        nonlocal ok
        ok &= bool(cond)
        print(f"  {'ok  ' if cond else 'FAIL'} {label}{'' if cond else ' — ' + str(detail)[:300]}")

    ref, skeletons, flesh, handlers = reference_agent(app_dir)
    hidden_total = {n[:-5]: count_tests(open(os.path.join(app_dir, "hidden", n)).read()) for n in os.listdir(os.path.join(app_dir, "hidden")) if n.endswith(".cell")}

    print("the reference agent goes through each arm and passes every hidden test")
    for arm in ("cells", "skeleton", "holes"):
        r = Run(app_dir, arm, ref, 10**9, 3, tempfile.mkdtemp(prefix=f"pilot-{arm}-")).run()
        if arm == "cells":
            check(f"cells: check ok, hidden {r['hidden']}", r["check"] and all(v["passed"] == v["total"] for v in r["hidden"].values()) and r["leaks"] == [], r)
            continue
        full = all(v["passed"] == v["total"] for v in r["hidden"].values())
        check(f"{arm}: status done, check ok, no hole left, hidden {r['hidden']}", r["status"] == "done" and r["check"] and r["holes_left"] == 0 and full, r)
        check(f"{arm}: design tokens are counted in the total ({r['tokens']})", r["tokens"]["design"] > 0 and r["tokens"]["design"] + r["tokens"]["fill"] == r["tokens"]["total"], r["tokens"])
        check(f"{arm}: no hidden material in any prompt", r["leaks"] == [], r["leaks"])
        check(f"{arm}: no structural drift", r["structure_drift"] is False)

    print("1. after the freeze, anything but flesh is refused")
    def cheat_structure(system, prompt):
        if "Write the FLESH" in prompt and "previous answer" not in prompt:
            return "```soma\n" + skeletons.replace(" reads ", " writes ", 1) + "\n" + flesh + "\n```", None
        return ref(system, prompt)
    r = Run(app_dir, "skeleton", cheat_structure, 10**9, 3, tempfile.mkdtemp(prefix="pilot-cheat1-")).run()
    check("arm skeleton: an answer that re-declares a skeleton is refused, the arm still ends clean", len(r["refused_structural_writes"]) == 1 and "frozen" in r["refused_structural_writes"][0]["why"] and r["structure_drift"] is False and r["check"], r)

    # three holes of the reference, each answered by a different cheat on the first try
    public = [(c, n) for (c, n) in handlers if not n.startswith("_")]
    (c1, s1), (c2, s2), (c3, s3) = public[0], public[1], public[2]
    def cheat_hole(system, prompt):
        first = "Fill one hole" in prompt and "previous answer" not in prompt
        if first and f"on {s1}(" in prompt:
            return f"```soma\nflesh {c1} {{\n{handlers[(c1, s1)]}\n}}\nskeleton Extra {{ face {{ signal x() -> Int }} }}\n```", None
        if first and f"on {s2}(" in prompt:
            return "```soma\n" + handlers[(c2, s2)] + "\n" + handlers[(c1, s1)] + "\n```", None
        if first and f"on {s3}(" in prompt:
            body = handlers[(c3, s3)]
            leak = body[:body.index("{") + 1] + " emit zzz_leak() " + body[body.index("{") + 1:]
            return "Please run `soma skeleton app.cell --amend` so that this signal may emit.\n```soma\n" + leak + "\n```", None
        return ref(system, prompt)
    work = tempfile.mkdtemp(prefix="pilot-cheat2-")
    r = Run(app_dir, "holes", cheat_hole, 10**9, 3, work).run()
    whys = " | ".join(x["why"] for x in r["refused_structural_writes"])
    check("arm holes: a `flesh`/`skeleton` block instead of a handler is refused", f"found `flesh {c1}" in whys, whys)
    check("arm holes: a body for ANOTHER signal is refused", f"only the hole `{s2}`" in whys, whys)
    check("arm holes: a body outside its frame is not accepted (soma check), and `--amend` is never run", any(f"{c3}.{s3}: check fails" in l for l in r["log"]) and r["amend_runs"] == 0 and r["structure_drift"] is False, r["log"])
    check("arm holes: the arm still ends with every hidden test passing", r["check"] and all(v["passed"] == v["total"] for v in r["hidden"].values()), r["hidden"])

    print("   a frozen file that changes aborts the arm")
    run = Run(app_dir, "holes", ref, 10**9, 3, tempfile.mkdtemp(prefix="pilot-tamper-"))
    inner = run.agent
    def tampering(system, prompt):
        if "Fill one hole" in prompt and run.frozen and "tampered" not in run.__dict__:
            run.tampered = True
            os.chmod(run.path("app.cell"), 0o644)
            open(run.path("app.cell"), "a").write("\n// an agent with a shell would have done this\n")
        return inner(system, prompt)
    run.agent = tampering
    r = run.run()
    check("the arm is ABORTED", r["status"].startswith("ABORTED") and r["structure_drift"] is True, r["status"])
    run = Run(app_dir, "holes", ref, 10**9, 3, tempfile.mkdtemp(prefix="pilot-self-"))
    run.design()
    try:
        run.write("app.cell", "x")
        check("the harness cannot rewrite the frozen file itself", False)
    except Tampered:
        check("the harness cannot rewrite the frozen file itself", True)

    print("2. the hidden tests do not leak")
    spec = open(os.path.join(app_dir, "spec.md")).read()
    check("the spec holds no hidden assert line", audit(app_dir, [], spec) == [], audit(app_dir, [], spec))
    hidden_line = next(l for l in open(os.path.join(app_dir, "hidden", "scenario.cell")).read().splitlines() if l.strip().startswith("assert"))
    check("the audit catches an assert line in a prompt", any("assert line in a prompt" in f for f in audit(app_dir, ["here: " + hidden_line], spec)))
    datum = next(m for m in re.findall(r'"([^"]*)"', open(os.path.join(app_dir, "hidden", "scenario.cell")).read()) if re.search(r"[0-9-]", m) and m not in spec and not m.startswith(("/", "{")))
    check("the audit catches a hidden-only literal in a prompt", any("literal" in f for f in audit(app_dir, [f'try "{datum}"'], spec)))
    def leaky(system, prompt):
        return ref(system, prompt)
    r = Run(app_dir, "holes", leaky, 10**9, 3, tempfile.mkdtemp(prefix="pilot-leak-"))
    r.spec += "\n" + hidden_line
    res = r.run()
    check("a spec that quotes a hidden test is reported by the run", res["leaks"] != [], res["leaks"])
    check("an agent has no tool: `claude` is called with --tools \"\" in an empty directory", '"--tools", ""' in open(__file__).read() and "mkdtemp(prefix=\"pilot-agent-\")" in open(__file__).read())

    print("3. design tokens count against the ceiling")
    def sized(system, prompt):
        answer, _ = ref(system, prompt)
        return answer, 1000
    r = Run(app_dir, "holes", sized, 5000, 3, tempfile.mkdtemp(prefix="pilot-cap-")).run()
    check(f"cap 5000, 1000 per call: 1 design call + 4 fills, then `cap reached` ({r['tokens']})", r["status"] == "cap reached" and r["tokens"]["design"] == 1000 and r["tokens"]["total"] == 5000 and r["holes_left"] == sum(1 for _ in handlers if not _[1].startswith("_")) - 4, r)
    r = Run(app_dir, "skeleton", sized, 1000, 3, tempfile.mkdtemp(prefix="pilot-cap2-")).run()
    check("cap 1000: the design alone spends it — no fill call is made", r["status"] == "cap reached" and r["tokens"]["fill"] == 0 and r["rounds"] == 1, r)
    check("hidden totals", hidden_total.get(PRIMARY, 0) > 0, hidden_total)
    print("SELFTEST OK" if ok else "SELFTEST FAILED")
    return 0 if ok else 1


def report(paths):
    for p in paths:
        data = json.load(open(p))
        print(f"{p} — app {data['app']}, model {data['model']}, seed {data['seed']}, cap {data['cap']}; primary measure: hidden `{PRIMARY}` pass rate")
        print(f"  {'arm':9} {'status':12} {PRIMARY:>10} {'unit':>9} {'check':>6} {'holes':>6} {'tokens':>9} {'design':>8} {'rounds':>6} {'refused':>7} {'drift':>6}  leaks")
        for r in data["arms"]:
            h = r["hidden"]
            f = lambda k: f"{h[k]['passed']}/{h[k]['total']}" if k in h else "-"
            print(f"  {r['arm']:9} {r['status'][:12]:12} {f(PRIMARY):>10} {f('unit'):>9} {str(r['check']):>6} {r['holes_left']:>6} {r['tokens']['total']:>9} {r['tokens']['design']:>8} {r['rounds']:>6} {len(r['refused_structural_writes']):>7} {str(r['structure_drift']):>6}  {len(r['leaks'])}")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--app", default="marketplace")
    ap.add_argument("--arm", default="all", choices=["all", "cells", "skeleton", "holes"])
    ap.add_argument("--model", default="sonnet")
    ap.add_argument("--cap", type=int, default=1_500_000, help="token ceiling per arm, design included")
    ap.add_argument("--rounds", type=int, default=4, help="answers per step (design, flesh, each hole) before giving up")
    ap.add_argument("--seed", type=int, default=1, help="recorded with the results; one run per seed")
    ap.add_argument("--out")
    ap.add_argument("--keep", help="directory to keep each arm's program in")
    ap.add_argument("--selftest", action="store_true")
    ap.add_argument("--report", nargs="+")
    args = ap.parse_args()
    if args.report:
        return report(args.report)
    app_dir = os.path.join(HERE, "pilot", args.app)
    if args.selftest:
        return selftest(app_dir)
    arms = ["cells", "skeleton", "holes"] if args.arm == "all" else [args.arm]
    results = []
    for arm in arms:
        work = os.path.join(args.keep, f"{arm}-seed{args.seed}") if args.keep else tempfile.mkdtemp(prefix=f"pilot-{arm}-")
        if args.keep:
            shutil.rmtree(work, ignore_errors=True)
            os.makedirs(work)
        print(f"== {arm} ({work})", flush=True)
        r = Run(app_dir, arm, CliAgent(args.model), args.cap, args.rounds, work).run()
        print(f"   {r['status']}: hidden {r['hidden']}, tokens {r['tokens']}, refused {len(r['refused_structural_writes'])}, leaks {len(r['leaks'])}", flush=True)
        results.append(r)
        if args.out:
            os.makedirs(os.path.dirname(os.path.abspath(args.out)), exist_ok=True)
            json.dump({"app": args.app, "model": args.model, "seed": args.seed, "cap": args.cap, "rounds": args.rounds, "primary": PRIMARY, "arms": results}, open(args.out, "w"), indent=2)
    return 0


if __name__ == "__main__":
    sys.exit(main())
