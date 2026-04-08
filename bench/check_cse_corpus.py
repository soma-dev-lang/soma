"""Verify auto-CSE detection rule against examples/cse_corpus.

The rule (see bench/auto_opts_study.md, candidate 3):
Within a single straight-line statement sequence, find call expressions
that:
  1. Call a [native] sibling handler (pure by construction in Soma) OR
     a known-pure builtin from a small allowlist
  2. Have the same syntactic form (same callee, same arg expressions)
  3. None of the args reference a variable that has been reassigned
     between the two call sites

If a handler contains 2+ such matching calls in the same straight-line
block, CSE fires.

Returns 'yes' (fires) or 'no'.
"""
import re, os, glob, sys

PURE_BUILTINS = {
    'sq', 'cube', 'gcd', 'is_prime', 'bit_test', 'shr', 'shl', 'band',
    'bor', 'bxor', 'bnot', 'sqrt', 'log', 'exp', 'pow', 'sin', 'cos',
    'abs', 'min', 'max', 'len', 'to_string', 'to_float', 'to_int',
}
IMPURE_BUILTINS = {'random', 'now_ms', 'rand_int'}

def strip_comments(s):
    return re.sub(r'//[^\n]*', '', s)

def find_handlers(src):
    out = []
    for m in re.finditer(r'on\s+(\w+)\s*\(([^)]*)\)\s*(\[[^\]]*\])?\s*\{', src):
        name = m.group(1)
        attrs = m.group(3) or ''
        is_native = 'native' in attrs
        params = []
        for p in m.group(2).split(','):
            p = p.strip()
            if not p: continue
            params.append(p.split(':')[0].strip())
        start = m.end(); depth = 1; j = start
        while j < len(src) and depth > 0:
            if src[j] == '{': depth += 1
            elif src[j] == '}': depth -= 1
            j += 1
        out.append((name, params, src[start:j-1], is_native))
    return out

def find_siblings(src):
    return {m.group(1) for m in re.finditer(r'on\s+(\w+)\s*\([^)]*\)', src)}

def split_top_statements(body):
    """Yield top-level statements; each statement is brace-aware."""
    stmts = []
    depth = 0
    cur = ''
    for c in body:
        if c == '{':
            depth += 1; cur += c
        elif c == '}':
            depth -= 1; cur += c
            if depth == 0:
                stmts.append(cur.strip()); cur = ''
        elif c == '\n' and depth == 0:
            if cur.strip():
                stmts.append(cur.strip()); cur = ''
        else:
            cur += c
    if cur.strip():
        stmts.append(cur.strip())
    return [s for s in stmts if s]

def find_calls_in_expr(expr):
    """Yield (callee, args_str) for top-level function calls in expr."""
    out = []
    i = 0
    while i < len(expr):
        m = re.search(r'\b(\w+)\s*\(', expr[i:])
        if not m: break
        callee = m.group(1)
        # skip language keywords
        if callee in ('if', 'while', 'for', 'return', 'let'):
            i += m.end()
            continue
        start = i + m.end()
        depth = 1; j = start
        while j < len(expr) and depth > 0:
            if expr[j] == '(': depth += 1
            elif expr[j] == ')': depth -= 1
            j += 1
        out.append((callee, expr[start:j-1].strip()))
        i = j
    return out

def find_referenced_vars(s):
    """Naive identifier extraction from an expression."""
    return set(re.findall(r'\b([a-zA-Z_]\w*)\b', s))

def find_reassigned_vars(stmts_window):
    """Vars assigned (not let-declared) in a window of stmts."""
    out = set()
    for s in stmts_window:
        # `name = expr` (assign, not `let name = expr`)
        m = re.match(r'(\w+)\s*=\s', s.lstrip('}').strip())
        if m and not s.lstrip('}').strip().startswith('let '):
            out.add(m.group(1))
    return out

def is_cse_eligible(name, body, siblings, is_native):
    if not is_native: return False
    stmts = split_top_statements(body)
    # Process the body's straight-line subsequences (no if/while/for mixing).
    # Conservative: only consider statements that are not control-flow.
    straight = []
    for s in stmts:
        if re.match(r'(if|while|for)\b', s):
            # break the run; check the run we have
            if check_run_for_cse(straight, siblings):
                return True
            straight = []
        else:
            straight.append(s)
    if check_run_for_cse(straight, siblings):
        return True
    return False

def check_run_for_cse(stmts, siblings):
    """Look for two identical pure calls in this straight-line run with no
    intervening reassignment of any of their referenced vars."""
    # Collect (call_form, stmt_index, referenced_vars) for every pure call.
    calls = []
    for idx, s in enumerate(stmts):
        for callee, args in find_calls_in_expr(s):
            if callee in IMPURE_BUILTINS:
                continue
            is_pure = (callee in siblings) or (callee in PURE_BUILTINS)
            if not is_pure:
                continue
            normalized = f"{callee}({args.strip()})"
            calls.append((normalized, idx, find_referenced_vars(args), s))
    # For each pair: same form, no var reassigned between them.
    for i in range(len(calls)):
        for j in range(i+1, len(calls)):
            form_i, idx_i, vars_i, _ = calls[i]
            form_j, idx_j, vars_j, _ = calls[j]
            if form_i != form_j:
                continue
            between = stmts[idx_i+1:idx_j+1]  # include j's own stmt's prior assigns? no
            reassigned = find_reassigned_vars(between)
            if vars_i & reassigned:
                continue
            return True
    return False

def find_expected(src):
    m = re.search(r'expected_fire:\s*(yes|no)', src)
    return m.group(1) if m else None

def check(path):
    raw = open(path).read()
    src = strip_comments(raw)
    expected = find_expected(raw)
    siblings = find_siblings(src)
    actual = 'no'
    for name, params, body, is_native in find_handlers(src):
        if name == 'run': continue
        if is_cse_eligible(name, body, siblings, is_native):
            actual = 'yes'
            break
    return expected, actual

if __name__ == '__main__':
    print(f"{'cell':35s} {'expected':10s} {'actual':10s} verdict")
    print('-' * 75)
    ok = 0; bad = 0
    for path in sorted(glob.glob('examples/cse_corpus/c*.cell')):
        name = os.path.basename(path).replace('.cell', '')
        expected, actual = check(path)
        if expected == actual:
            ok += 1; verdict = '✓'
        else:
            bad += 1; verdict = '✗ MISMATCH'
        print(f"{name:35s} {expected or '?':10s} {actual:10s} {verdict}")
    print('-' * 75)
    print(f"  match: {ok} / {ok + bad}")
    sys.exit(0 if bad == 0 else 1)
