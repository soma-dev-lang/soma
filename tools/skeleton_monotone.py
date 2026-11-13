#!/usr/bin/env python3
"""What `soma verify` proves on a skeleton must stay proven once it is filled.

For every .cell file that passes `soma check`:

    soma skeleton FILE  ->  its skeleton: the same structure, every signal a hole

`soma verify` on that skeleton reads each hole as its frame. Every `✓` it
prints is then a claim about EVERY body that conforms to the frames — and
the program itself is one such body. So each `✓` of the skeleton must be a
`✓` of the program: one that is not (a `✗`, a `⚠`, or missing) is a proof
that held only because a hole was empty.

    python3 tools/skeleton_monotone.py [--soma PATH] [--jobs N] [FILES…]
"""
import argparse, collections, concurrent.futures, os, re, shutil, subprocess, sys, tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ENV = dict(os.environ, SOMA_LLM_MOCK="fixed:ok", NO_COLOR="1")


def key(line):
    """The claim of a verify line, without its justification."""
    text = line.strip()[1:].strip()
    text = text.split(" — ")[0]
    return re.sub(r"\s*\([^()]*\)\s*$", "", text).strip()


def proven(out):
    """Multiset of the claims printed with a ✓, per section."""
    got, section = collections.Counter(), ""
    for line in out.splitlines():
        s = line.strip()
        if s.startswith("State machine") or s.endswith(":") and not s.startswith(("✓", "✗", "⚠", "·")):
            section = s
        elif s.startswith("✓"):
            got[(section, key(s))] += 1
    return got


def one(soma, path):
    rel = os.path.relpath(path, ROOT)
    src_dir, name = os.path.dirname(path), os.path.basename(path)
    if subprocess.run([soma, "check", name], cwd=src_dir, env=ENV, capture_output=True).returncode != 0:
        return rel, "skip", []
    full = subprocess.run([soma, "verify", name], cwd=src_dir, env=ENV, capture_output=True, text=True)
    sk = subprocess.run([soma, "skeleton", name], cwd=src_dir, env=ENV, capture_output=True, text=True)
    if sk.returncode != 0 or "skeleton " not in sk.stdout:
        return rel, "nocell", []
    with tempfile.TemporaryDirectory() as tmp:
        open(os.path.join(tmp, name), "w", encoding="utf-8").write(sk.stdout)
        toml = os.path.join(src_dir, "soma.toml")
        if os.path.exists(toml):
            shutil.copy(toml, tmp)
        if subprocess.run([soma, "check", name], cwd=tmp, env=ENV, capture_output=True).returncode != 0:
            return rel, "underivable", []
        hole = subprocess.run([soma, "verify", name], cwd=tmp, env=ENV, capture_output=True, text=True)
    a, b = proven(hole.stdout + hole.stderr), proven(full.stdout + full.stderr)
    lost = [f"{sec} ✓ {k}" for (sec, k), n in a.items() if b[(sec, k)] < n]
    return rel, ("BROKEN" if lost else "ok"), lost


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--soma", default=os.path.join(ROOT, "compiler/target/release/soma"))
    ap.add_argument("--jobs", type=int, default=8)
    ap.add_argument("files", nargs="*")
    args = ap.parse_args()
    files = [os.path.abspath(f) for f in args.files] or [
        os.path.join(ROOT, f) for f in subprocess.run(["git", "ls-files", "*.cell"], cwd=ROOT, capture_output=True, text=True).stdout.split()]
    # (the reproducers of the review rounds are adversarial programs: each is a test of
    # compiler/tests/skeleton.rs, not a program these harnesses hold an invariant on)
    if not args.files:
        files = [f for f in files if "/tests/fixtures/skeleton_review/" not in f]
    counts, bad = {}, []
    with concurrent.futures.ThreadPoolExecutor(args.jobs) as pool:
        for rel, status, lost in pool.map(lambda f: one(args.soma, f), files):
            counts[status] = counts.get(status, 0) + 1
            if lost:
                bad.append((rel, lost))
    for rel, lost in sorted(bad):
        print(rel)
        for l in lost[:6]:
            print("   ", l[:200])
    print(", ".join(f"{k}: {v}" for k, v in sorted(counts.items())))
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
