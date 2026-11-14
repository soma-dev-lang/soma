#!/usr/bin/env python3
"""Check the wrong -> right pairs of docs/site/errors/<CODE>/.

Each directory holds:
    wrong.cell   a program on which `soma check` reports <CODE>
    right.cell   the same program fixed: `soma check` passes, <CODE> gone
    note.md      two to four sentences: the rule, why Soma has it, the fix
    soma.toml    (optional) the manifest both programs are checked with
    wrong.soma.toml / right.soma.toml
                 (optional) a manifest for one side only (when the mistake is
                 in the manifest itself: E0112)
    example.cell (instead of wrong/right, for a code that is a note, not a
                 fault: W0010, W0017) a clean program on which check reports it
    unreachable  (optional, instead of the .cell files) why no program
                 can show this code from `soma check`

    python3 tools/check_error_pairs.py              # every pair
    python3 tools/check_error_pairs.py E0028 E0104  # some
    SOMA=compiler/target/release/soma python3 tools/check_error_pairs.py

Returns the problems; exits 1 if there is any.
"""
import json
import os
import shutil
import subprocess
import sys
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ERRORS = os.path.join(ROOT, "docs", "site", "errors")
SOMA = os.path.abspath(os.environ.get("SOMA", os.path.join(ROOT, "compiler", "target", "release", "soma")))


def check(soma, d, name):
    r = subprocess.run([soma, "check", "--json", name], cwd=d, capture_output=True, text=True, timeout=60)
    try:
        j = json.loads(r.stdout)
    except ValueError:
        return None, (r.stdout + r.stderr).strip()
    return j, None


def stage(src, d, side):
    """Copy the pair's files into folder d as `side` ("wrong", "right" or
    "example") sees them: its own <side>.soma.toml becomes soma.toml."""
    for f in os.listdir(src):
        p = os.path.join(src, f)
        if not os.path.isfile(p) or f.startswith("."):
            continue
        if f.endswith(".soma.toml"):
            if f == side + ".soma.toml":
                shutil.copyfile(p, os.path.join(d, "soma.toml"))
            continue
        if f == "soma.toml" and os.path.exists(os.path.join(src, side + ".soma.toml")):
            continue
        shutil.copyfile(p, os.path.join(d, f))


def check_pair(code, soma=SOMA):
    """[] when the pair holds, else the problems (strings)."""
    src = os.path.join(ERRORS, code)
    if not os.path.isdir(src):
        return [f"{code}: no directory"]
    files = {f for f in os.listdir(src) if os.path.isfile(os.path.join(src, f)) and not f.startswith('.')}
    if "note.md" not in files:
        return [f"{code}: no note.md"]
    if "unreachable" in files:
        return []
    if "example.cell" in files:
        with tempfile.TemporaryDirectory() as d:
            stage(src, d, "example")
            j, err = check(soma, d, "example.cell")
        if j is None:
            return [f"{code}: example.cell — no JSON from soma check: {err[:200]}"]
        got = [x.get("code") for x in j.get("errors", []) + j.get("warnings", []) + j.get("notes", []) if isinstance(x, dict)]
        if j.get("errors"):
            return [f"{code}: example.cell has errors {[x.get('code') for x in j['errors']]}"]
        return [] if code in got else [f"{code}: example.cell reports {sorted(set(c for c in got if c)) or 'nothing'}, not {code}"]
    probs = []
    for need in ("wrong.cell", "right.cell"):
        if need not in files:
            probs.append(f"{code}: no {need}")
    if probs:
        return probs
    with tempfile.TemporaryDirectory() as d:
        stage(src, d, "wrong")
        j, err = check(soma, d, "wrong.cell")
        if j is None:
            return [f"{code}: wrong.cell — no JSON from soma check: {err[:200]}"]
        got = [x.get("code") for x in j.get("errors", []) + j.get("warnings", []) + j.get("notes", []) if isinstance(x, dict)]
        if code not in got:
            probs.append(f"{code}: wrong.cell reports {sorted(set(c for c in got if c)) or 'nothing'}, not {code}")
    with tempfile.TemporaryDirectory() as d:
        stage(src, d, "right")
        j, err = check(soma, d, "right.cell")
        if j is None:
            return probs + [f"{code}: right.cell — no JSON from soma check: {err[:200]}"]
        got = [x.get("code") for x in j.get("errors", []) + j.get("warnings", [])]
        if j.get("errors"):
            probs.append(f"{code}: right.cell has errors {[x.get('code') for x in j['errors']]}: {j['errors'][0].get('message', '')[:160]}")
        elif code in got:
            probs.append(f"{code}: right.cell still reports {code}")
    return probs


def main():
    codes = sys.argv[1:] or sorted(c for c in os.listdir(ERRORS) if os.path.isdir(os.path.join(ERRORS, c)))
    probs = []
    for c in codes:
        p = check_pair(c)
        probs += p
        print(("ok   " if not p else "FAIL ") + c)
    for p in probs:
        print("  " + p)
    sys.exit(1 if probs else 0)


if __name__ == "__main__":
    main()
