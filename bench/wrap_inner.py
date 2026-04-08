"""Rewrite each bench/py/<name>.py to wrap its top-level workload in inner().

Heuristic: everything after the last `def` or `import` line that contains a
function definition is moved into a `def workload(): ...` function and the
file ends with `inner(workload)`.

We're conservative: only rewrite files that don't already use `inner(`.
"""
import os, re, sys, ast

DIR = os.path.join(os.path.dirname(__file__), 'py')
SKIP = {'_inner.py'}

for name in sorted(os.listdir(DIR)):
    if not name.endswith('.py') or name in SKIP:
        continue
    path = os.path.join(DIR, name)
    src = open(path).read()
    if 'inner(' in src and 'from _inner' in src:
        continue  # already wrapped
    # Parse to find top-level statements vs definitions
    try:
        tree = ast.parse(src)
    except SyntaxError:
        print(f'  SKIP (parse error): {name}')
        continue
    # Split top-level into "header" (imports + def + class) and "tail" (calls)
    header_nodes = []
    tail_nodes = []
    for node in tree.body:
        if isinstance(node, (ast.Import, ast.ImportFrom, ast.FunctionDef, ast.ClassDef, ast.AsyncFunctionDef)):
            header_nodes.append(node)
        else:
            tail_nodes.append(node)
    if not tail_nodes:
        print(f'  SKIP (no top-level statements): {name}')
        continue
    # Render header verbatim from original source by line ranges
    lines = src.splitlines()
    # Find the last header end-line
    if header_nodes:
        last_header_end = max((getattr(n, 'end_lineno', n.lineno) for n in header_nodes))
        header_src = '\n'.join(lines[:last_header_end])
        tail_src = '\n'.join(lines[last_header_end:]).strip()
    else:
        header_src = ''
        tail_src = src.strip()
    # Build new file
    indented_tail = '\n'.join('    ' + l if l else '' for l in tail_src.splitlines())
    new_src = (
        ('from _inner import inner\n' if header_src else 'from _inner import inner\n')
        + (header_src + '\n\n' if header_src else '')
        + 'def workload():\n'
        + (indented_tail if indented_tail else '    pass')
        + '\n\ninner(workload)\n'
    )
    open(path, 'w').write(new_src)
    print(f'  wrapped: {name}')
