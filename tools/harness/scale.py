#!/usr/bin/env python3
"""How does the toolchain's time grow with the size of the program?

The corpus is made of small files, so a pass that is quadratic or cubic in
the number of cells or handlers shows nowhere in it (`soma verify` rebuilt
the call graph for every cell: 400 cells took 25 s). Each probe below
generates one program at size N and one at 2N, times one command on each,
and reports the seconds at 2N and the growth exponent

    exponent = log2( t(2N) / t(N) )        1 linear · 2 quadratic · 3 cubic

    python3 tools/harness/scale.py [--soma PATH] [--only NAME] [--keep]

Prints {"probes": {name: {"n", "seconds", "exponent"}}}. A probe that passes
its time limit reports "seconds": null. Standard library only.
"""
import argparse, json, math, os, subprocess, sys, tempfile, time

LIMIT = 120  # seconds, per command


def cells(n, chain):
    out = []
    for i in range(n):
        ret = f"C{i+1}.f(n) + 1" if chain and i < n - 1 else "n"
        out.append(f"cell C{i} {{\n  face {{ signal f(n: Int) -> Int }}\n  memory {{ m: Map<String, Int> [persistent]\n    invariant m >= 0 }}\n"
                   f"  on f(n: Int) {{ require n >= 0 else Bad\n    m.set(\"k\", n)\n    return {ret} }}\n}}")
    return "\n".join(out)


def handlers(n, chain):
    out = ["cell H {", "  face {"] + [f"    signal h{i}(n: Int) -> Int" for i in range(n)] + ["  }"]
    for i in range(n):
        out.append(f"  on h{i}(n: Int) {{ return {f'h{i+1}(n) + 1' if chain and i < n - 1 else f'n + {i}'} }}")
    return "\n".join(out + ["}"])


NEST = """cell N {
  face { signal list(n: Int) -> Int
         signal map(n: Int) -> Int }
  on list(n: Int) {
    let v = []
    for i in range(0, n) { v = [v] }
    return 1
  }
  on map(n: Int) {
    let v = map("a", 1)
    for i in range(0, n) { v = map("a", v) }
    return 1
  }
}"""

# name: (N, source at size n, command at size n)
PROBES = {
    # many cells — a large application
    "verify_cells_independent": (400, lambda n: cells(n, False), lambda n: ["verify", "p.cell"]),
    "verify_cells_call_chain": (400, lambda n: cells(n, True), lambda n: ["verify", "p.cell"]),
    "check_cells_independent": (400, lambda n: cells(n, False), lambda n: ["check", "p.cell"]),
    # one wide cell
    "check_handlers_independent": (2000, lambda n: handlers(n, False), lambda n: ["check", "p.cell"]),
    "check_handlers_call_chain": (1000, lambda n: handlers(n, True), lambda n: ["check", "p.cell"]),
    "verify_handlers_independent": (1000, lambda n: handlers(n, False), lambda n: ["verify", "p.cell"]),
    # values at run time: a structure built by wrapping (a linked list, a tree)
    "run_nested_list": (8000, lambda n: NEST, lambda n: ["run", "p.cell", "list", str(n)]),
    "run_nested_map": (2000, lambda n: NEST, lambda n: ["run", "p.cell", "map", str(n)]),
}


def timed(soma, d, src, cmd):
    open(os.path.join(d, "p.cell"), "w").write(src)
    best = None
    for _ in range(2):  # the better of two: the first run pays the page cache
        t = time.perf_counter()
        try:
            subprocess.run([soma] + cmd, capture_output=True, cwd=d, timeout=LIMIT)
        except subprocess.TimeoutExpired:
            return None
        took = time.perf_counter() - t
        best = took if best is None else min(best, took)
    return best


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--soma", default=os.environ.get("SOMA", "compiler/target/release/soma"))
    ap.add_argument("--only")
    ap.add_argument("--keep", action="store_true", help="work in the current directory")
    a = ap.parse_args()
    soma = os.path.abspath(a.soma)
    out = {}
    with tempfile.TemporaryDirectory() as tmp:
        d = os.getcwd() if a.keep else tmp
        for name, (n, src, cmd) in PROBES.items():
            if a.only and a.only != name:
                continue
            t1 = timed(soma, d, src(n), cmd(n))
            t2 = timed(soma, d, src(2 * n), cmd(2 * n)) if t1 is not None else None
            # below 50 ms the process start dominates: no exponent to read
            exp = round(math.log2(t2 / t1), 2) if t1 and t2 and t2 >= 0.05 else None
            out[name] = {"n": 2 * n, "seconds": round(t2, 3) if t2 is not None else None, "exponent": exp}
    print(json.dumps({"probes": out}, indent=1))


if __name__ == "__main__":
    main()
