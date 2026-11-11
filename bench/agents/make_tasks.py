#!/usr/bin/env python3
"""Build the agent-convergence tasks from the verified corpus (spec A7).

Each corpus program becomes one task: the scenario (its header comment),
the contracts to implement (`soma describe --faces`), and its `cell test`
— given to the agent as the acceptance tests. The corpus program itself is
the reference solution: every task is solvable by construction.

    python3 bench/agents/make_tasks.py            # writes bench/agents/tasks.json
"""
import json
import os
import re
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
SOMA = os.environ.get("SOMA", os.path.join(ROOT, "compiler/target/release/soma"))
CORPUS = os.path.join(ROOT, "examples/corpus")


def split_test_cell(source):
    """(program without its test cell, the test cell). Brace matching that
    skips strings and comments."""
    m = re.search(r"^cell test \w+\s*\{", source, re.M)
    if not m:
        return None
    i, depth, n = m.end(), 1, len(source)
    while i < n and depth:
        c = source[i]
        if source.startswith("//", i):
            i = source.find("\n", i)
            if i < 0:
                i = n
            continue
        if source.startswith('"""', i):
            i = source.find('"""', i + 3) + 3
            continue
        if c == '"':
            i += 1
            while i < n and source[i] != '"':
                i += 2 if source[i] == "\\" else 1
        elif c == "{":
            depth += 1
        elif c == "}":
            depth -= 1
        i += 1
    return source[:m.start()] + source[i:], source[m.start():i]


def header(source):
    lines = []
    for line in source.splitlines():
        if line.startswith("//"):
            text = line[2:].strip()
            if set(text) <= set("=-─ "):
                continue
            lines.append(text)
        elif line.strip() == "" and lines:
            lines.append("")
        elif line.strip():
            break
    return "\n".join(lines).strip()


def main():
    tasks = []
    for domain in sorted(os.listdir(CORPUS)):
        d = os.path.join(CORPUS, domain)
        if not os.path.isdir(d) or domain.startswith("_"):
            continue
        for name in sorted(os.listdir(d)):
            if not name.endswith(".cell"):
                continue
            path = os.path.join(d, name)
            source = open(path).read()
            parts = split_test_cell(source)
            if parts is None:
                continue
            program, tests = parts
            faces = subprocess.run([SOMA, "describe", path, "--faces"], capture_output=True, text=True).stdout
            # the first line names the file; the test cell's own contract is not part of the task
            faces = "\n".join(l for l in faces.splitlines()[1:]).strip()
            faces = re.split(r"\n(?=cell test )", faces)[0].strip() if "cell test " in faces else faces
            toml = os.path.join(d, name[:-5] + ".soma.toml")
            tasks.append({
                "id": f"{domain}/{name[:-5]}",
                "domain": domain,
                "scenario": header(source),
                "contracts": faces,
                "tests": tests.strip(),
                "reference": program.strip() + "\n",
                "soma_toml": open(toml).read() if os.path.exists(toml) else None,
            })
    out = os.path.join(os.path.dirname(os.path.abspath(__file__)), "tasks.json")
    with open(out, "w") as f:
        json.dump(tasks, f, indent=1, ensure_ascii=False)
    print(f"{len(tasks)} tasks → {os.path.relpath(out, ROOT)}", file=sys.stderr)


if __name__ == "__main__":
    main()
