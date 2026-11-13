#!/usr/bin/env python3
"""Split every program into skeleton + flesh and compare it with the original.

For each .cell file that passes `soma check`:

    soma skeleton FILE --split   ->  the same program as `skeleton` + `flesh`

and the split program must pass `soma check` with no frame error, give the
same `soma test` tally and the same `soma verify` verdict. A frame inferred
from a body and then enforced on that body has to hold: any difference is a
bug in the inference, the conformance check or the split itself.

    python3 tools/split_roundtrip.py [--soma PATH] [--jobs N] [FILES…]

Without FILES: every tracked .cell file of the repository.
"""
import argparse, concurrent.futures, os, re, shutil, subprocess, sys, tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ENV = dict(os.environ, SOMA_LLM_MOCK="fixed:ok", NO_COLOR="1")


def run(soma, args, cwd, timeout=120):
    try:
        p = subprocess.run([soma] + args, cwd=cwd, env=ENV, capture_output=True, text=True, timeout=timeout)
        return p.returncode, p.stdout + p.stderr
    except subprocess.TimeoutExpired:
        return -9, "timeout"


def tally(out):
    m = re.findall(r"(\d+) tests?: (\d+) passed, (\d+) failed", out)
    return m[-1] if m else ("no tests" if "no test cells" in out else out.strip().splitlines()[-1:] )


def verdict(out):
    lines = [l for l in out.splitlines() if l.startswith("VERIFY")]
    return lines[-1] if lines else "?"


def one(soma, path):
    rel = os.path.relpath(path, ROOT)
    src_dir = os.path.dirname(path)
    name = os.path.basename(path)
    code, _ = run(soma, ["check", name], src_dir)
    if code != 0:
        return rel, "skip", ""
    with tempfile.TemporaryDirectory() as tmp:
        a, b = os.path.join(tmp, "a"), os.path.join(tmp, "b")
        # the file with what it may import or read: its directory (no build
        # output, no data), twice
        ignore = shutil.ignore_patterns(".soma_data", ".soma_env", "target", "node_modules", "*.db", ".git")
        text = open(path, encoding="utf-8").read()
        if re.search(r"^\s*use\s", text, re.M) or os.path.exists(os.path.join(src_dir, "soma.toml")):
            # a project directory; a loose file among hundreds is copied alone
            if len(os.listdir(src_dir)) > 60:
                os.makedirs(a); shutil.copy(path, a)
                for extra in ("soma.toml", "lib"):
                    e = os.path.join(src_dir, extra)
                    if os.path.isdir(e): shutil.copytree(e, os.path.join(a, extra), ignore=ignore)
                    elif os.path.exists(e): shutil.copy(e, a)
            else:
                shutil.copytree(src_dir, a, ignore=ignore)
        else:
            os.makedirs(a); shutil.copy(path, a)
        shutil.copytree(a, b)
        code, out = run(soma, ["skeleton", name, "--split"], a)
        if code != 0:
            return rel, "FAIL", "split: " + out.strip()[-300:]
        p = subprocess.run([soma, "skeleton", name, "--split"], cwd=a, env=ENV, capture_output=True, text=True)
        unframed = "no frame can declare it" in p.stderr
        open(os.path.join(b, name), "w", encoding="utf-8").write(p.stdout)
        code, out = run(soma, ["check", name], b)
        if code != 0:
            errs = [l for l in out.splitlines() if l.startswith("error")]
            if unframed and errs and all("computed at run time" in e or "no single state machine" in e for e in errs):
                return rel, "unframed", errs[0][:200]
            return rel, "FAIL", "check: " + (errs[0] if errs else out.strip()[-300:])
        if "skeleton " not in p.stdout:
            return rel, "nocell", ""
        ta, tb = tally(run(soma, ["test", name], a)[1]), tally(run(soma, ["test", name], b)[1])
        if ta != tb:
            return rel, "FAIL", f"test: {ta} became {tb}"
        va, vb = verdict(run(soma, ["verify", name], a)[1]), verdict(run(soma, ["verify", name], b)[1])
        if va != vb:
            return rel, "FAIL", f"verify: {va!r} became {vb!r}"
        return rel, "ok", ""


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--soma", default=os.path.join(ROOT, "compiler/target/release/soma"))
    ap.add_argument("--jobs", type=int, default=8)
    ap.add_argument("files", nargs="*")
    args = ap.parse_args()
    files = [os.path.abspath(f) for f in args.files] or [
        os.path.join(ROOT, f) for f in subprocess.run(["git", "ls-files", "*.cell"], cwd=ROOT, capture_output=True, text=True).stdout.split()]
    counts, failures = {}, []
    with concurrent.futures.ThreadPoolExecutor(args.jobs) as pool:
        for rel, status, detail in pool.map(lambda f: one(args.soma, f), files):
            counts[status] = counts.get(status, 0) + 1
            if status in ("FAIL", "unframed"):
                failures.append((status, rel, detail))
    for status, rel, detail in sorted(failures):
        print(f"{status:9} {rel}: {detail}")
    print(", ".join(f"{k}: {v}" for k, v in sorted(counts.items())))
    return 1 if counts.get("FAIL") else 0


if __name__ == "__main__":
    sys.exit(main())
