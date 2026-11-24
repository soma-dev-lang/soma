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


LIMIT = 120  # seconds, each command (--limit): a file that does not finish is counted, never passed

# Files where a rule the skeleton proves is not proven with the bodies, and why
# (one per line, `path  # reason`). A file listed here that is broken is "known";
# one listed that is no longer broken, or broken and not listed, fails the run:
# the list says what is true today, and nothing else.
KNOWN = os.path.join(ROOT, "tools/skeleton_monotone.known")


def known():
    if not os.path.exists(KNOWN):
        return {}
    out = {}
    for line in open(KNOWN, encoding="utf-8"):
        line = line.strip()
        if line and not line.startswith("#"):
            path, _, why = line.partition("#")
            out[path.strip()] = why.strip()
    return out


def one(soma, path):
    try:
        return checked(soma, path)
    except subprocess.TimeoutExpired:
        return os.path.relpath(path, ROOT), "timeout", []


def checked(soma, path):
    rel = os.path.relpath(path, ROOT)
    src_dir, name = os.path.dirname(path), os.path.basename(path)
    if subprocess.run([soma, "check", name], cwd=src_dir, env=ENV, capture_output=True, timeout=LIMIT).returncode != 0:
        return rel, "skip", []
    full = subprocess.run([soma, "verify", name], cwd=src_dir, env=ENV, capture_output=True, text=True, timeout=LIMIT)
    sk = subprocess.run([soma, "skeleton", name], cwd=src_dir, env=ENV, capture_output=True, text=True, timeout=LIMIT)
    if sk.returncode != 0 or "skeleton " not in sk.stdout:
        return rel, "nocell", []
    with tempfile.TemporaryDirectory() as tmp:
        open(os.path.join(tmp, name), "w", encoding="utf-8").write(sk.stdout)
        toml = os.path.join(src_dir, "soma.toml")
        if os.path.exists(toml):
            shutil.copy(toml, tmp)
        if subprocess.run([soma, "check", name], cwd=tmp, env=ENV, capture_output=True, timeout=LIMIT).returncode != 0:
            return rel, "underivable", []
        hole = subprocess.run([soma, "verify", name], cwd=tmp, env=ENV, capture_output=True, text=True, timeout=LIMIT)
    a, b = proven(hole.stdout + hole.stderr), proven(full.stdout + full.stderr)
    lost = [f"{sec} ✓ {k}" for (sec, k), n in a.items() if b[(sec, k)] < n]
    return rel, ("BROKEN" if lost else "ok"), lost


def main():
    global LIMIT
    ap = argparse.ArgumentParser()
    ap.add_argument("--soma", default=os.path.join(ROOT, "compiler/target/release/soma"))
    ap.add_argument("--jobs", type=int, default=8)
    ap.add_argument("--limit", type=int, default=LIMIT, help="seconds for each command")
    ap.add_argument("files", nargs="*")
    args = ap.parse_args()
    LIMIT = args.limit
    args.soma = os.path.abspath(args.soma)  # each program runs in its own directory
    files = [os.path.abspath(f) for f in args.files] or [
        os.path.join(ROOT, f) for f in subprocess.run(["git", "ls-files", "*.cell"], cwd=ROOT, capture_output=True, text=True).stdout.split()]
    # (the reproducers of the review rounds are adversarial programs: each is a test of
    # compiler/tests/skeleton.rs, not a program these harnesses hold an invariant on)
    if not args.files:
        files = [f for f in files if "/tests/fixtures/skeleton_review/" not in f]
    counts, bad, late = {}, [], []
    with concurrent.futures.ThreadPoolExecutor(args.jobs) as pool:
        for rel, status, lost in pool.map(lambda f: one(args.soma, f), files):
            counts[status] = counts.get(status, 0) + 1
            if status == "timeout":
                late.append(rel)
            if lost:
                bad.append((rel, lost))
    listed = known()
    seen = dict(bad)
    new = [(rel, lost) for rel, lost in sorted(bad) if rel not in listed]
    for rel, lost in new:
        print(rel)
        for l in lost[:6]:
            print("   ", l[:200])
    # (only where the whole set was run: a listed file not run is not "fixed")
    stale = [] if args.files else sorted(rel for rel in listed if rel not in seen and rel not in late)
    for rel in stale:
        print(f"stale     {rel}: listed in tools/skeleton_monotone.known and no longer broken — remove the line")
    if seen:
        counts["BROKEN"] = len(new)
        counts["known"] = len(seen) - len(new)
        if not new:
            del counts["BROKEN"]
    for rel in sorted(late):
        print(f"timeout   {rel}: a command did not finish in {LIMIT} s")
    print(", ".join(f"{k}: {v}" for k, v in sorted(counts.items())))
    return 1 if new or late or stale else 0


if __name__ == "__main__":
    sys.exit(main())
