"""Verify the auto-iteration detection rule against examples/iter_corpus.

The rule (see bench/auto_opts_study.md, candidate 1):
A `[native]` handler `f(p1..pn)` is auto-iter eligible iff:
  1. It is [native]
  2. All params are scalar (Int/Float/Bool — not String)
  3. Body contains exactly ONE self-call to f
  4. That call is in tail position: the body's last top-level statement
     is `return f(args)` (the return expression IS the call, nothing else)
  5. The call is NOT inside a while/for loop

Returns 'yes' if the rule fires, 'no' otherwise.
"""
import re, os, glob, sys

def strip_comments(s):
    return re.sub(r'//[^\n]*', '', s)

def find_handlers(src):
    out = []
    for m in re.finditer(r'on\s+(\w+)\s*\(([^)]*)\)\s*(\[[^\]]*\])?\s*\{', src):
        name = m.group(1)
        attrs = m.group(3) or ''
        is_native = 'native' in attrs
        params_raw = m.group(2)
        params = []
        for p in params_raw.split(','):
            p = p.strip()
            if not p: continue
            parts = p.split(':')
            pname = parts[0].strip()
            ptype = parts[1].strip() if len(parts) > 1 else ''
            params.append((pname, ptype))
        start = m.end(); depth = 1; j = start
        while j < len(src) and depth > 0:
            if src[j] == '{': depth += 1
            elif src[j] == '}': depth -= 1
            j += 1
        out.append((name, params, src[start:j-1], is_native))
    return out

def split_top_statements(body):
    """Walk the body and yield top-level statements as raw substrings.
    Brace-aware. Splits on newlines outside of any nested block."""
    stmts = []
    depth = 0
    cur = ''
    for c in body:
        if c == '{':
            depth += 1
            cur += c
        elif c == '}':
            depth -= 1
            cur += c
            if depth == 0:
                stmts.append(cur.strip())
                cur = ''
        elif c == '\n' and depth == 0:
            if cur.strip():
                stmts.append(cur.strip())
                cur = ''
        else:
            cur += c
    if cur.strip():
        stmts.append(cur.strip())
    return [s for s in stmts if s]

def count_self_calls(body, name):
    return len(re.findall(rf'\b{re.escape(name)}\s*\(', body))

def is_inside_loop(body, name):
    """Check whether any self-call to name is inside a while/for loop."""
    # Find all `while ... { ... }` and `for ... { ... }` blocks; check if
    # they contain a self-call.
    for kw in ('while', 'for'):
        for m in re.finditer(rf'\b{kw}\b[^{{]*\{{', body):
            start = m.end()
            depth = 1; j = start
            while j < len(body) and depth > 0:
                if body[j] == '{': depth += 1
                elif body[j] == '}': depth -= 1
                j += 1
            block = body[start:j-1]
            if re.search(rf'\b{re.escape(name)}\s*\(', block):
                return True
    return False

def last_statement_is_self_tail(body, name):
    """Check whether the body's last top-level statement is exactly
    `return name(args)` with nothing else after the closing paren."""
    stmts = split_top_statements(body)
    if not stmts:
        return False
    last = stmts[-1]
    # Match 'return <name>(...)' optionally followed by whitespace
    m = re.match(rf'return\s+{re.escape(name)}\s*\(', last)
    if not m:
        return False
    # Find the matching close paren
    rest = last[m.end():]
    depth = 1; j = 0
    while j < len(rest) and depth > 0:
        if rest[j] == '(': depth += 1
        elif rest[j] == ')': depth -= 1
        j += 1
    after = rest[j:].strip()
    return after == ''

def is_iter_eligible(name, params, body, is_native):
    if not is_native: return False
    # Param types: must all be Int/Float/Bool (no String)
    for pname, ptype in params:
        if ptype == 'String':
            return False
    # Exactly one self-call
    if count_self_calls(body, name) != 1:
        return False
    # That call must NOT be inside a loop
    if is_inside_loop(body, name):
        return False
    # And the body's last statement must be `return self(args)`
    if not last_statement_is_self_tail(body, name):
        return False
    return True

def find_expected(src):
    m = re.search(r'expected_fire:\s*(yes|no)', src)
    return m.group(1) if m else None

def check(path):
    src = strip_comments(open(path).read())
    expected = find_expected(open(path).read())
    actual = 'no'
    for name, params, body, is_native in find_handlers(src):
        # Skip the `run` handler — it's the entry point, not a candidate.
        if name == 'run':
            continue
        if is_iter_eligible(name, params, body, is_native):
            actual = 'yes'
            break
    return expected, actual

if __name__ == '__main__':
    print(f"{'cell':35s} {'expected':10s} {'actual':10s} verdict")
    print('-' * 75)
    ok = 0; bad = 0
    for path in sorted(glob.glob('examples/iter_corpus/i*.cell')):
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
