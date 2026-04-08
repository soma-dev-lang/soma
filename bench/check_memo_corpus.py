"""Run the auto-memo detection rule against examples/memo_corpus/*.cell.

For each cell:
  1. Read the `expected_memo: yes/no` header annotation.
  2. Run the detection rule on every [native] handler.
  3. Report agreement/disagreement.

The rule (see proposal): a handler is auto-memo eligible iff it has
≥2 self-recursive calls in the same body, AND every argument of every
self-call is recursively one of:
  - a parameter unchanged
  - param ± small_literal (|literal| ≤ 5)
  - a literal in [-2^60, 2^60]
  - another self-call whose own args are simple
"""
import re, os, glob, sys

def find_expected(src):
    m = re.search(r'expected_memo:\s*(yes|no)', src)
    return m.group(1) if m else None

def find_handlers(src):
    """Yield (name, [param_names], body)."""
    i = 0
    while True:
        m = re.search(r'\bon\s+(\w+)\s*\(([^)]*)\)\s*\[[^\]]*native[^\]]*\]\s*\{', src[i:])
        if not m: return
        name = m.group(1)
        params = []
        for p in m.group(2).split(','):
            p = p.strip()
            if not p: continue
            params.append(p.split(':')[0].strip())
        start = i + m.end()
        depth = 1
        j = start
        while j < len(src) and depth > 0:
            if src[j] == '{': depth += 1
            elif src[j] == '}': depth -= 1
            j += 1
        yield name, params, src[start:j-1]
        i = j

def split_args(s):
    out, depth, cur = [], 0, ''
    for c in s:
        if c == ',' and depth == 0:
            out.append(cur); cur = ''
        else:
            if c == '(': depth += 1
            elif c == ')': depth -= 1
            cur += c
    if cur.strip(): out.append(cur)
    return out

def is_simple_arg(arg, params, fn_name):
    arg = arg.strip()
    if re.fullmatch(r'-?\d+', arg):
        n = int(arg)
        return -(1 << 60) <= n <= (1 << 60)
    if arg in params:
        return True
    # param − small_literal (subtraction only — climbing rejected)
    m = re.fullmatch(r'(\w+)\s*-\s*(\d+)', arg)
    if m and m.group(1) in params and int(m.group(2)) <= 5:
        return True
    # param − other_param (partition-style: p(n - k, k)). The first
    # operand must be a parameter; the second must also be a parameter
    # of this function, ensuring the result is bounded by the initial
    # arg space rather than depending on a runtime local.
    m = re.fullmatch(r'(\w+)\s*-\s*(\w+)', arg)
    if m and m.group(1) in params and m.group(2) in params:
        return True
    # nested self-call
    m = re.fullmatch(rf'{re.escape(fn_name)}\s*\((.*)\)', arg, re.DOTALL)
    if m:
        return all(is_simple_arg(a, params, fn_name) for a in split_args(m.group(1)))
    return False

def self_calls(body, fn_name):
    out = []
    i = 0
    while True:
        m = re.search(rf'\b{re.escape(fn_name)}\s*\(', body[i:])
        if not m: return out
        start = i + m.end()
        depth = 1
        j = start
        while j < len(body) and depth > 0:
            if body[j] == '(': depth += 1
            elif body[j] == ')': depth -= 1
            j += 1
        out.append(body[start:j-1])
        i = j

def memo_eligible(body, name, params):
    calls = self_calls(body, name)
    if len(calls) < 2:
        return False
    return all(
        all(is_simple_arg(a, params, name) for a in split_args(c))
        for c in calls
    )

def check_cell(path):
    src = open(path).read()
    expected = find_expected(src)
    actual = None
    for name, params, body in find_handlers(src):
        if memo_eligible(body, name, params):
            actual = 'yes'
            break
    if actual is None:
        actual = 'no'
    return expected, actual

print(f"{'cell':35s} {'expected':10s} {'actual':10s} {'verdict'}")
print('-' * 75)

ok = 0
mismatches = 0
for path in sorted(glob.glob('examples/memo_corpus/m*.cell')):
    name = os.path.basename(path).replace('.cell', '')
    expected, actual = check_cell(path)
    if expected is None:
        verdict = "?? (no annotation)"
        mismatches += 1
    elif expected == actual:
        verdict = "✓"
        ok += 1
    else:
        verdict = "✗ MISMATCH"
        mismatches += 1
    print(f"{name:35s} {expected or '?':10s} {actual:10s} {verdict}")

print('-' * 75)
print(f"  match: {ok} / {ok + mismatches}")
sys.exit(0 if mismatches == 0 else 1)
