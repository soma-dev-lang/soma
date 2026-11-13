#!/usr/bin/env python3
"""Are the frames sound? Compare them with what handlers actually do.

`soma skeleton FILE --json` gives, for every signal, the frame the checker
infers (or the skeleton declares): the slots it reads and writes, the
signals and builtins it calls, the machine edges it takes, what it emits.

`SOMA_EFFECT_TRACE=<file> soma test FILE` makes the interpreter log every
effect a handler really has while the program's test cells run.

Each observed effect must be inside the frame of the signal that made it
(the nearest signal on the handler stack: a helper's effects are its
caller's). One that is not is an effect the static analysis missed — a
soundness bug in `checker/frame.rs`.

    python3 tools/frame_oracle.py [--soma PATH] [--jobs N] [FILES…]

Without FILES: every tracked .cell file that has a test cell.
"""
import argparse, concurrent.futures, json, os, re, shutil, subprocess, sys, tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def one(soma, path):
    rel = os.path.relpath(path, ROOT)
    src_dir, name = os.path.dirname(path), os.path.basename(path)
    text = open(path, encoding="utf-8").read()
    if not re.search(r"^\s*cell\s+test\s", text, re.M):
        return rel, "notest", []
    with tempfile.TemporaryDirectory() as tmp:
        work = os.path.join(tmp, "w")
        ignore = shutil.ignore_patterns(".soma_data", ".soma_env", "target", "node_modules", "*.db", ".git")
        if (re.search(r"^\s*use\s", text, re.M) or os.path.exists(os.path.join(src_dir, "soma.toml"))) and len(os.listdir(src_dir)) <= 60:
            shutil.copytree(src_dir, work, ignore=ignore)
        else:
            os.makedirs(work); shutil.copy(path, work)
            for extra in ("soma.toml", "lib"):
                e = os.path.join(src_dir, extra)
                if os.path.isdir(e): shutil.copytree(e, os.path.join(work, extra), ignore=ignore)
                elif os.path.exists(e): shutil.copy(e, work)
        env = dict(os.environ, SOMA_LLM_MOCK="fixed:ok", NO_COLOR="1")
        p = subprocess.run([soma, "skeleton", name, "--json"], cwd=work, env=env, capture_output=True, text=True)
        if p.returncode != 0:
            return rel, "skip", []
        skeleton = json.loads(p.stdout)
        trace = os.path.join(tmp, "trace.tsv")
        try:
            subprocess.run([soma, "test", name], cwd=work, env=dict(env, SOMA_EFFECT_TRACE=trace), capture_output=True, text=True, timeout=180)
        except subprocess.TimeoutExpired:
            return rel, "timeout", []
        if not os.path.exists(trace):
            return rel, "notrace", []
        cells = {c["name"]: c for c in skeleton["cells"]}
        frames = {(c["name"], s["name"]): s["frame"] for c in skeleton["cells"] for s in c["signals"]}
        slots = {c["name"]: {m["name"] for m in c["memory"]} for c in skeleton["cells"]}
        missed = set()
        for line in open(trace, encoding="utf-8", errors="replace"):
            parts = line.rstrip("\n").split("\t")
            if len(parts) != 3:
                continue
            stack, kind, what = parts
            handlers = [tuple(h.split(".", 1)) for h in stack.split(">") if "." in h]
            if kind == "call":
                callee = tuple(what.split(".", 1))
                if callee not in frames:
                    continue  # a helper: its effects are checked against its caller's frame
            bone = next((h for h in reversed(handlers) if h in frames), None)
            if bone is None:
                continue  # a test rule, or the runtime itself
            cell, sig = bone
            f = frames[bone]
            # `resume()` / `deliver()` run the rest of a WAITING handler inside the
            # caller: the frame says so by naming the builtin (an opaque one), and
            # what the continuation does is that handler's, not this one's
            if any(b in f["calls"] for b in ("resume", "deliver")):
                continue
            ok = True
            if kind == "call":
                # a handler named at run time is run by the builtin the frame names
                # (a listener run by an `emit` the frame declares is that emit)
                ok = ((what.split(".", 1)[1] if callee[0] == cell else what) in f["calls"] or callee == bone
                      or callee[1] in f["emits"]
                      or any(b in f["calls"] for b in ("delegate", "horde", "vote", "resume", "deliver")))
            elif kind == "read":
                ok = what in f["reads"] or what in f["writes"] or what not in slots.get(cell, ())
            elif kind == "write":
                ok = what in f["writes"] or what not in slots.get(cell, ())
            elif kind == "builtin":
                # `delegate("Cell", "h", …)` with literal names is the call `Cell.h` in the frame
                # (`await approve(…)` is the builtin `approve`, declared as `await_approve`)
                ok = what in f["calls"] or what == "delegate" or ("await_" + what) in f["calls"]
            elif kind == "move":
                ok = any(m["to"] in (what, "*") for m in f["moves"]) or f["dynamic_moves"]
            elif kind == "edge":
                # the edge itself: a frame that grants the target from another state does not grant this
                src, dst = what.split(">", 1)
                ok = any(m["to"] in (dst, "*") and m["from"] in (src, "*") for m in f["moves"]) or f["dynamic_moves"]
            elif kind == "mread":
                ok = bool(f["moves"]) or any(r not in slots.get(cell, ()) for r in f["reads"]) or f["dynamic_moves"]
            elif kind == "emit":
                ok = what in f["emits"]
            if not ok:
                missed.add(f"{cell}.{sig}: {kind} {what}")
        return rel, ("MISSED" if missed else "ok"), sorted(missed)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--soma", default=os.path.join(ROOT, "compiler/target/release/soma"))
    ap.add_argument("--jobs", type=int, default=8)
    ap.add_argument("files", nargs="*")
    args = ap.parse_args()
    files = [os.path.abspath(f) for f in args.files] or [
        os.path.join(ROOT, f) for f in subprocess.run(["git", "ls-files", "*.cell"], cwd=ROOT, capture_output=True, text=True).stdout.split()]
    counts, bad = {}, []
    with concurrent.futures.ThreadPoolExecutor(args.jobs) as pool:
        for rel, status, missed in pool.map(lambda f: one(args.soma, f), files):
            counts[status] = counts.get(status, 0) + 1
            if missed:
                bad.append((rel, missed))
    for rel, missed in sorted(bad):
        print(rel)
        for m in missed[:8]:
            print("   ", m)
    print(", ".join(f"{k}: {v}" for k, v in sorted(counts.items())))
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
