"""Verify the auto-tabulation detection rule against examples/tab_corpus.

The rule (see bench/auto_opts_study.md, candidate 2):
A `[native]` handler `f(p1..pn)` is auto-tab eligible iff:
  1. It is auto-memo eligible (≥2 self-calls, all-Int params, arity 1-3)
  2. The body is "tab-simple": zero or more `if cond { return base }`
     guards followed by exactly one `return <expr>` whose expression is
     only literals + params + arithmetic + self-calls
  3. Every self-call's i-th argument is `p_i` (unchanged) or `p_i - k`
     where k ≥ 0 is a small literal (≤ 5). Position-preserving — the
     i-th arg refers to the i-th param.
  4. At least one self-call has at least one strict shrink (k ≥ 1) on
     at least one parameter.

Returns 'yes' if the rule fires.
"""
import re, os, glob, sys
sys.path.insert(0, os.path.dirname(__file__))
import check_memo_corpus as memo

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
            parts = p.split(':')
            params.append((parts[0].strip(), parts[1].strip() if len(parts) > 1 else ''))
        start = m.end(); depth = 1; j = start
        while j < len(src) and depth > 0:
            if src[j] == '{': depth += 1
            elif src[j] == '}': depth -= 1
            j += 1
        out.append((name, params, src[start:j-1], is_native))
    return out

def split_top_statements(body):
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

def parse_arg_form(arg, params, fn_name):
    """Return ('param', name, 0) or ('shrink', name, k) or None.
    Position-irrelevant for now — caller checks position.
    """
    arg = arg.strip()
    if arg in params:
        return ('param', arg, 0)
    m = re.fullmatch(rf'(\w+)\s*-\s*(\d+)', arg)
    if m and m.group(1) in params:
        k = int(m.group(2))
        if 0 <= k <= 5:
            return ('shrink', m.group(1), k)
    return None

def is_tab_simple_body(body, name, params):
    """Body must be: guard* + one return whose expression contains only
    literals/params/arithmetic/self-calls (no let, no nested ifs, no
    sibling calls, no while/for)."""
    stmts = split_top_statements(body)
    if not stmts: return False, []
    # All but the last must be `if <cond> { return <simple-expr> }`
    # OR `if <cond> { return <simple-expr> } else { return ... }` or
    # nested guards collapsed... we'll be strict and only accept the
    # plain `if cond { return X }` shape.
    self_calls = []
    for s in stmts[:-1]:
        if not re.match(r'if\s+.*\{', s):
            return False, []
        # Body of the if must be exactly one `return <const-or-param-expr>`
        # (not containing self-calls)
        body_match = re.search(r'\{(.+)\}\s*$', s, re.DOTALL)
        if not body_match: return False, []
        inner = body_match.group(1).strip()
        if not re.match(r'return\s', inner):
            return False, []
        # No self-calls in guard
        if re.search(rf'\b{re.escape(name)}\s*\(', inner):
            return False, []
    # Last statement must be `return <expr>` containing self-calls
    last = stmts[-1]
    m = re.match(r'return\s+(.+)$', last, re.DOTALL)
    if not m: return False, []
    expr = m.group(1).strip()
    # Must NOT contain `let`, `while`, `for`, or sibling calls.
    # Allow: literals, identifiers, +, -, *, /, %, parens, self-calls.
    # Strip self-calls first to check the rest.
    cleaned = expr
    self_calls_args = []
    i = 0
    while True:
        m2 = re.search(rf'\b{re.escape(name)}\s*\(', cleaned[i:])
        if not m2: break
        start = i + m2.end()
        depth = 1; j = start
        while j < len(cleaned) and depth > 0:
            if cleaned[j] == '(': depth += 1
            elif cleaned[j] == ')': depth -= 1
            j += 1
        self_calls_args.append(cleaned[start:j-1])
        # Replace with a placeholder
        cleaned = cleaned[:i + m2.start()] + '0' + cleaned[j:]
        i = i + m2.start() + 1
    # Now `cleaned` must contain only allowed tokens (no other identifiers
    # that could be sibling calls). The simplest test: no other `(`.
    if '(' in cleaned:
        return False, []
    # And no `let` keyword
    if re.search(r'\blet\b', cleaned):
        return False, []
    return True, self_calls_args

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

def is_tab_eligible(name, params_with_types, body, is_native):
    if not is_native: return False
    # Must satisfy auto-memo first
    pnames = [p[0] for p in params_with_types]
    # Reuse memo's checker (it works on stripped src)
    # Inline the memo check here for clarity
    memo_handler_match = (
        len(pnames) >= 1 and len(pnames) <= 3
        and all(t == 'Int' for _, t in params_with_types)
    )
    if not memo_handler_match: return False
    calls = memo.self_calls(body, name)
    if len(calls) < 2: return False
    if not all(all(memo.is_simple_arg(a, pnames, name) for a in memo.split_args(c)) for c in calls):
        return False
    # Now the tab-specific checks
    ok, simple_args = is_tab_simple_body(body, name, pnames)
    if not ok: return False
    # Every self-call: each arg must be position-preserving param-or-shrink
    # AND each call must shrink at least one parameter strictly (k >= 1).
    # Otherwise the call creates a self-loop in tab fill order.
    for args_str in simple_args:
        args = split_args(args_str)
        if len(args) != len(pnames): return False
        call_has_shrink = False
        for i, arg in enumerate(args):
            form = parse_arg_form(arg, pnames, name)
            if form is None: return False
            tag, ref, k = form
            # Position-preserving: must reference the i-th param name
            if ref != pnames[i]: return False
            if k >= 1: call_has_shrink = True
        if not call_has_shrink:
            return False
    return True

def find_expected(src):
    m = re.search(r'expected_fire:\s*(yes|no)', src)
    return m.group(1) if m else None

def check(path):
    raw = open(path).read()
    src = strip_comments(raw)
    expected = find_expected(raw)
    actual = 'no'
    for name, params, body, is_native in find_handlers(src):
        if name == 'run': continue
        if is_tab_eligible(name, params, body, is_native):
            actual = 'yes'
            break
    return expected, actual

if __name__ == '__main__':
    print(f"{'cell':35s} {'expected':10s} {'actual':10s} verdict")
    print('-' * 75)
    ok = 0; bad = 0
    for path in sorted(glob.glob('examples/tab_corpus/t*.cell')):
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
