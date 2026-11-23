#!/usr/bin/env python3
"""Does `soma verify` print the same thing as the reference binary?

    python3 tools/harness/verify_same.py --ref tools/harness/.ref/soma [--soma PATH] [--jobs N] [FILES…]

On every tracked .cell file (or FILES): `verify` with both binaries, in the
file's own directory, and the two outputs compared byte for byte. A change
of speed must not be a change of words: a count of explored states, a
witness run, the order of the lines are all part of what `verify` says.

What the reference printed is kept under tools/harness/.ref/verify/ (keyed by
the reference binary and the file's content), so only the new binary runs
again. A file the reference cannot finish in its time limit has nothing to
be compared with: it is counted as `ref_timeout`, never as the same.

Prints JSON: {"files", "same", "different": [...], "ref_timeout", "timeout": [...]}.
Exit 1 when a file differs, or when the new binary times out where the
reference did not.
"""
import argparse, concurrent.futures, hashlib, json, os, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
CACHE = os.path.join(HERE, ".ref", "verify")
ENV = dict(os.environ, SOMA_LLM_MOCK="fixed:ok", NO_COLOR="1")
LIMIT = 60
# the reference needs minutes on these; they are the point of the comparison
LONG = {"site/cases/orbite/src/app.cell": 900}


def sha(path):
    return hashlib.sha256(open(path, "rb").read()).hexdigest()[:16]


def verify(soma, path, limit):
    try:
        p = subprocess.run([soma, "verify", os.path.basename(path)], cwd=os.path.dirname(path), env=ENV, capture_output=True, timeout=limit)
        return f"exit {p.returncode}\n".encode() + p.stdout + b"\n--stderr--\n" + p.stderr
    except subprocess.TimeoutExpired:
        return None


def one(soma, ref, ref_id, path):
    rel = os.path.relpath(path, ROOT)
    limit = LONG.get(rel, LIMIT)
    # (a program is its file and what it imports: the directory's listing stands for the rest)
    key = hashlib.sha256((ref_id + rel + sha(path)).encode()).hexdigest()[:24]
    kept = os.path.join(CACHE, key)
    if os.path.exists(kept):
        want = open(kept, "rb").read()
    else:
        out = verify(ref, path, limit)
        want = b"TIMEOUT" if out is None else out
        os.makedirs(CACHE, exist_ok=True)
        open(kept, "wb").write(want)
    if want == b"TIMEOUT":
        return rel, "ref_timeout"
    got = verify(soma, path, limit)
    if got is None:
        return rel, "timeout"
    return rel, ("same" if got == want else "different")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--soma", default=os.path.join(ROOT, "compiler/target/release/soma"))
    ap.add_argument("--ref", required=True)
    ap.add_argument("--jobs", type=int, default=8)
    ap.add_argument("--show", action="store_true", help="print the first differing file's two outputs")
    ap.add_argument("files", nargs="*")
    a = ap.parse_args()
    soma, ref = os.path.abspath(a.soma), os.path.abspath(a.ref)
    ref_id = sha(ref)
    files = [os.path.abspath(f) for f in a.files] or [
        os.path.join(ROOT, f) for f in subprocess.run(["git", "ls-files", "*.cell"], cwd=ROOT, capture_output=True, text=True).stdout.split()
        if "/tests/fixtures/skeleton_review/" not in f]
    # the long ones alone, after the rest: they are timed against a limit
    long_ones = [f for f in files if os.path.relpath(f, ROOT) in LONG]
    files = [f for f in files if f not in long_ones]
    res = {"files": len(files) + len(long_ones), "same": 0, "different": [], "ref_timeout": 0, "timeout": []}
    with concurrent.futures.ThreadPoolExecutor(a.jobs) as pool:
        done = list(pool.map(lambda f: one(soma, ref, ref_id, f), files))
    done += [one(soma, ref, ref_id, f) for f in long_ones]
    for rel, status in done:
        if status == "same": res["same"] += 1
        elif status == "ref_timeout": res["ref_timeout"] += 1
        else: res[status].append(rel)
    print(json.dumps(res, indent=1))
    if a.show and res["different"]:
        path = os.path.join(ROOT, res["different"][0])
        got = verify(soma, path, 900) or b"TIMEOUT"
        want = verify(ref, path, 900) or b"TIMEOUT"
        import difflib
        sys.stdout.writelines(list(difflib.unified_diff(want.decode(errors="replace").splitlines(True), got.decode(errors="replace").splitlines(True), "reference", "now"))[:80])
    return 1 if res["different"] or res["timeout"] else 0


if __name__ == "__main__":
    sys.exit(main())
