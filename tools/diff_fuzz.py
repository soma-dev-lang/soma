#!/usr/bin/env python3
"""Differential fuzzer: interpreter vs [native] (spec B1).

Generates random handlers in the native-eligible subset, each written twice
in one cell — `fK_i` interpreted, `fK_n` [native], the SAME body — and a
driver that calls both with the same arguments and compares what comes
back: the value with its type (`Int:7` is not `Float:7.0`), or the error
kind. The interpreter is the reference semantics; any difference is a bug.

    python3 tools/diff_fuzz.py --files 50 --seed 1          # a campaign
    python3 tools/diff_fuzz.py --files 1 --seed 7 --keep    # keep the program

One file holds --funcs handlers × --inputs argument tuples and costs one
native compile, so a campaign's size is files × funcs × inputs comparisons.
Exit status 1 when a divergence was found (each is printed with the handler
source needed to reproduce it).
"""
import argparse
import concurrent.futures as cf
import json
import os
import random
import shutil
import subprocess
import sys
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SOMA = os.environ.get("SOMA", os.path.join(ROOT, "compiler/target/release/soma"))

INT_ARGS = [0, 1, -1, 2, 3, -7, 13, 100, 255, -256, 65537, 2**31 - 1, -(2**31), 2**53 + 1,
            2**62, 2**63 - 1, -(2**63) + 1, 10**18, 999983]
FLOAT_ARGS = ["0.0", "1.0", "-1.0", "1.5", "-2.25", "0.1", "3.0", "1e10", "-7.5", "1e-3", "123456.789"]


class Gen:
    """One handler body. Locals are typed for the handler's whole life (a
    native local keeps the type of its `let`), and every name is bound
    before any read."""

    def __init__(self, rng, features):
        self.r = rng
        self.features = features
        self.ints = ["a", "b"]
        self.floats = ["x"]
        self.fresh = 0
        self.loop_depth = 0

    def name(self, prefix):
        self.fresh += 1
        return f"{prefix}{self.fresh}"

    # ── expressions ────────────────────────────────────────────────────
    def int_expr(self, d=0):
        r = self.r
        if d > 3 or r.random() < 0.3:
            if r.random() < 0.55:
                return r.choice(self.ints)
            return str(r.choice([0, 1, 2, 3, 5, 7, 10, 16, 31, 64, 100, 1000, 65536, 4294967296, 9223372036854775807]))
        k = r.random()
        a, b = self.int_expr(d + 1), self.int_expr(d + 1)
        # a LITERAL zero divisor is refused at check in native code (rustc
        # would not compile it): only computed zeros are compared
        if b == "0" and 0.56 <= k < 0.72:
            b = "b"
        if k < 0.22:
            return f"({a} + {b})"
        if k < 0.40:
            return f"({a} - {b})"
        if k < 0.56:
            # no product inside a loop: `v = v * v` nine times is a number
            # of 2^24 bits, and the run is spent multiplying it
            return f"({a} * {b})" if self.loop_depth == 0 else f"({a} + {b})"
        if k < 0.64:
            return f"({a} % {b})"
        if k < 0.72:
            return f"idiv({a}, {b})"
        if k < 0.78:
            return f"abs({a})"
        if k < 0.84:
            return f"{r.choice(['min', 'max'])}({a}, {b})"
        if k < 0.88 and "bits" in self.features:
            return f"{r.choice(['band', 'bor', 'bxor'])}({a}, {b})"
        if k < 0.91 and "bits" in self.features and self.loop_depth == 0:
            return f"{r.choice(['shl', 'shr'])}({a}, {r.choice([0, 1, 3, 7, 31, 62, 63, 64, 70])})"
        if k < 0.94 and "bits" in self.features:
            return f"gcd({a}, {b})"
        if k < 0.97 and "conv" in self.features:
            return f"{r.choice(['floor', 'ceil', 'round'])}({self.float_expr(d + 1)})"
        return f"(0 - {a})"

    def float_expr(self, d=0):
        r = self.r
        if d > 3 or r.random() < 0.3:
            if r.random() < 0.5:
                return r.choice(self.floats)
            return r.choice(["0.0", "1.0", "0.5", "2.0", "-3.25", "10.0", "1e6", "0.001"])
        k = r.random()
        a, b = self.float_expr(d + 1), self.float_expr(d + 1)
        if k < 0.25:
            return f"({a} + {b})"
        if k < 0.45:
            return f"({a} - {b})"
        if k < 0.65:
            return f"({a} * {b})"
        if k < 0.75 and self.loop_depth > 0:
            return f"({a} - {b})"
        if k < 0.75:
            return f"({a} / {'x' if b == '0.0' else b})"
        if k < 0.82:
            return f"abs({a})"
        if k < 0.88:
            return f"{r.choice(['min', 'max'])}({a}, {b})"
        if k < 0.93 and "conv" in self.features:
            return f"to_float({self.int_expr(d + 1)})"
        if k < 0.97 and "math" in self.features:
            return f"sqrt(abs({a}))"
        if "quotient" in self.features:
            # an Int / Int in a Float expression: exact or not, a number
            den = self.int_expr(d + 1)
            return f"(({self.int_expr(d + 1)} / {'b' if den == '0' else den}) + 0.0)"
        return f"({a} * 0.5)"

    def bool_expr(self, d=0):
        r = self.r
        k = r.random()
        op = r.choice(["<", "<=", ">", ">=", "==", "!="])
        if d < 2 and k < 0.15:
            return f"({self.bool_expr(d + 1)} {r.choice(['&&', '||'])} {self.bool_expr(d + 1)})"
        if k < 0.2:
            return f"!({self.bool_expr(d + 1)})" if d < 2 else "true"
        if k < 0.75:
            return f"{self.int_expr(2)} {op} {self.int_expr(2)}"
        if k < 0.9:
            return f"{self.float_expr(2)} {op} {self.float_expr(2)}"
        # mixed Int / Float comparison is exact in every backend
        return f"{self.int_expr(2)} {op} {self.float_expr(2)}"

    # ── statements ─────────────────────────────────────────────────────
    def block(self, depth, n):
        out = []
        # names bound inside a block stay in it
        ints, floats = list(self.ints), list(self.floats)
        for _ in range(n):
            out.extend(self.stmt(depth))
        self.ints, self.floats = ints, floats
        return out

    def stmt(self, depth):
        r = self.r
        pad = "    " * (depth + 2)
        k = r.random()
        if k < 0.30:
            n = self.name("i")
            line = f"{pad}let {n} = {self.int_expr()}"
            self.ints.append(n)
            return [line]
        if k < 0.42:
            n = self.name("f")
            line = f"{pad}let {n} = {self.float_expr()}"
            self.floats.append(n)
            return [line]
        if k < 0.58:
            # only locals are assigned: a parameter keeps its declared type
            locals_ = [v for v in self.ints if v not in ("a", "b") and not v.startswith(("k", "w"))]
            if locals_:
                v = r.choice(locals_)
                return [f"{pad}{v} = {self.int_expr()}"]
            return []
        if k < 0.64:
            locals_ = [v for v in self.floats if v != "x"]
            if locals_:
                v = r.choice(locals_)
                return [f"{pad}{v} = {self.float_expr()}"]
            return []
        if k < 0.80 and depth < 2:
            out = [f"{pad}if {self.bool_expr()} {{"]
            out += self.block(depth + 1, r.randint(1, 3))
            if r.random() < 0.5:
                out.append(f"{pad}}} else {{")
                out += self.block(depth + 1, r.randint(1, 2))
            out.append(f"{pad}}}")
            return out
        if k < 0.90 and depth < 2 and self.loop_depth < 2:
            v = self.name("k")
            lo, hi = r.choice([0, 1, -2]), r.choice([0, 1, 3, 6, 9])
            self.loop_depth += 1
            self.ints.append(v)
            out = [f"{pad}for {v} in range({lo}, {hi}) {{"]
            out += self.block(depth + 1, r.randint(1, 3))
            out.append(f"{pad}}}")
            self.ints.remove(v)
            self.loop_depth -= 1
            return out
        if k < 0.95 and depth < 2 and self.loop_depth < 2:
            v = self.name("w")
            self.loop_depth += 1
            out = [f"{pad}let {v} = 0", f"{pad}while {v} < {r.choice([1, 2, 5, 8])} {{"]
            self.ints.append(v)
            body = self.block(depth + 1, r.randint(1, 2))
            self.ints.remove(v)
            out += body
            out.append(f"{pad}    {v} = {v} + 1")
            out.append(f"{pad}}}")
            self.loop_depth -= 1
            return out
        if depth > 0 and r.random() < 0.5:
            return [f"{pad}return {self.ret_expr()}"]
        return []

    def ret_expr(self):
        return self.int_expr() if self.kind == "Int" else self.float_expr()

    def handler(self, kind):
        self.kind = kind
        body = self.block(0, self.r.randint(2, 6))
        body.append(f"        return {self.ret_expr()}")
        return "\n".join(body)


def program(seed, funcs, inputs, features):
    rng = random.Random(seed)
    lines = ["cell F {"]
    calls = []
    sources = {}
    for k in range(funcs):
        kind = "Int" if rng.random() < 0.7 else "Float"
        body = Gen(rng, features).handler(kind)
        sources[k] = body
        lines.append(f"    on f{k}_i(a: Int, b: Int, x: Float) {{\n{body}\n    }}")
        lines.append(f"    on f{k}_n(a: Int, b: Int, x: Float) [native] {{\n{body}\n    }}")
        for _ in range(inputs):
            a, b, x = rng.choice(INT_ARGS), rng.choice(INT_ARGS), rng.choice(FLOAT_ARGS)
            calls.append((k, a, b, x))
    lines.append("    on _sig(r: Map) {")
    # a refusal native code announces as its own limit (an Int past 64 bits
    # inside a Float expression, an exact quotient past 2^53) is not a wrong
    # answer: it is counted apart
    lines.append('        if r.error != () && (contains(r.error, "[native]") || contains(r.error, "native code types")) { return "REFUSED" }')
    lines.append('        if r.error != () { return "ERR:" + r.kind }')
    lines.append('        return type_of(r.value) + ":" + to_string(r.value)')
    lines.append("    }")
    # one driver per handler: a handler body is bounded in size
    for k in range(funcs):
        lines.append(f"    on _drive{k}() {{")
        lines.append("        let bad = []")
        for (kk, a, b, x) in calls:
            if kk != k:
                continue
            args = f"{a}, {b}, {x}"
            lines.append(f"        let ri = _sig(try {{ f{k}_i({args}) }})")
            lines.append(f"        let rn = _sig(try {{ f{k}_n({args}) }})")
            lines.append(f'        if ri != rn {{ bad = push(bad, (if rn == "REFUSED" {{ "~" }} else {{ "" }}) + "f{k}({args}): interpreter " + ri + " | native " + rn) }}')
        lines.append("        return bad")
        lines.append("    }")
    lines.append("    on main() {")
    lines.append("        let bad = []")
    for k in range(funcs):
        lines.append(f"        bad = concat(bad, _drive{k}())")
    lines.append("        return bad")
    lines.append("    }")
    lines.append("}")
    return "\n".join(lines) + "\n", sources


def run_one(seed, funcs, inputs, features, keep):
    src, sources = program(seed, funcs, inputs, features)
    d = tempfile.mkdtemp(prefix=f"soma_fuzz_{seed}_")
    path = os.path.join(d, "app.cell")
    with open(path, "w") as f:
        f.write(src)
    try:
        r = subprocess.run([SOMA, "run", "--fresh", path, "main"], capture_output=True, text=True, timeout=180, cwd=d)
    except subprocess.TimeoutExpired:
        return seed, "timeout", [], d
    out = r.stdout.strip().splitlines()
    last = out[-1] if out else ""
    try:
        bad = json.loads(last)
    except Exception:
        tail = (r.stderr or r.stdout)[-1500:]
        return seed, "no result", [tail], d
    found = []
    for line in bad:
        refused = line.startswith("~")
        line = line.lstrip("~")
        k = int(line[1:line.index("(")])
        found.append((line, None if refused else sources[k]))
    if not found and not keep:
        shutil.rmtree(d, ignore_errors=True)
    return seed, "ok", found, d


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--files", type=int, default=10)
    ap.add_argument("--funcs", type=int, default=30)
    ap.add_argument("--inputs", type=int, default=12)
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--jobs", type=int, default=4)
    ap.add_argument("--features", default="bits,conv,math,quotient",
                    help="comma list: bits (band/shl/gcd), conv (floor/to_float), math (sqrt), quotient (Int / Int)")
    ap.add_argument("--keep", action="store_true", help="keep every generated program")
    ap.add_argument("--json", action="store_true")
    a = ap.parse_args()
    features = set(f for f in a.features.split(",") if f)
    total = a.files * a.funcs * a.inputs
    divergences, broken, refusals = [], [], []
    with cf.ThreadPoolExecutor(a.jobs) as ex:
        futs = [ex.submit(run_one, a.seed + i, a.funcs, a.inputs, features, a.keep) for i in range(a.files)]
        for fu in cf.as_completed(futs):
            seed, status, found, d = fu.result()
            if status != "ok":
                broken.append((seed, status, found, d))
                continue
            for line, body in found:
                if body is None:
                    refusals.append((seed, line))
                else:
                    divergences.append((seed, line, body, d))
    if a.json:
        print(json.dumps({
            "comparisons": total, "files": a.files, "seed": a.seed, "features": sorted(features),
            "divergences": [{"seed": s, "case": l, "handler": b} for s, l, b, _ in divergences],
            "native_refusals": len(refusals),
            "broken": [{"seed": s, "status": st, "detail": f[:1]} for s, st, f, _ in broken],
        }, indent=2))
    else:
        for seed, status, found, d in broken:
            print(f"seed {seed}: {status} ({d})")
            for t in found:
                print("   ", t.replace("\n", "\n    "))
        seen = set()
        for seed, line, body, d in sorted(divergences):
            print(f"DIVERGENCE seed {seed}: {line}")
            if (seed, body) not in seen:
                seen.add((seed, body))
                print(body)
                print(f"    ({d}/app.cell)")
        print(f"{total} comparisons in {a.files} file(s): {len(divergences)} divergence(s), "
              f"{len(refusals)} native refusal(s) (announced limits, not wrong answers), {len(broken)} file(s) did not run")
    sys.exit(1 if divergences or broken else 0)


if __name__ == "__main__":
    main()
