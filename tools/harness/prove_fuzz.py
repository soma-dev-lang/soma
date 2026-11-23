#!/usr/bin/env python3
"""Is `soma verify` sound, and how often does it fail a correct program?

Generates random handlers `f(n: Int, k: Int) -> Int` — lets, `require`s,
early returns, nested `if`s, arithmetic and integer builtins — and runs each
one on EVERY input its `requires` admits (n in -7..5, k in -3..9) to get the
true range [lo, hi] of what it returns. Then asks `soma verify` about four
`ensures` clauses per handler:

    result >= lo + 1      false      a `✓ proven` here is a FALSE PROOF  (unsound);
                                     a `✗` counts in `refuted` — the bugs verify names
    result <= hi - 1      false      idem
    result >= lo          true       a `✗` here FAILS A CORRECT PROGRAM  (false_fail)
    result <= hi          true       idem; a `✓` counts in `proven`

    python3 tools/harness/prove_fuzz.py --seeds 2-21 --bodies 40
    python3 tools/harness/prove_fuzz.py --seeds 7 --keep     # keep s7.cell / v7.cell

Prints one JSON object: {"bodies", "unsound", "false_fail", "proven", "refuted", "cases"}.
`false_fail` at 0 is worth nothing alone — a verify that never says `✗` has it:
`refuted` (of 2 × bodies false clauses) is the number that must not fall.
Exit 1 when unsound > 0. Standard library only.
"""
import argparse, json, os, random, re, subprocess, sys, tempfile

NL, NH, KL, KH = -7, 5, -3, 9
F2 = ['idiv', 'floor_div', 'mod', 'min', 'max', 'div_round']


def lit(v):
    return str(v) if v >= 0 else f"(0 - {-v})"


class Gen:
    def __init__(self, seed):
        self.r = random.Random(seed)

    def expr(self, d, vs):
        r = self.r.random()
        if d == 0 or r < 0.3:
            return self.r.choice(vs + vs + [lit(self.r.randint(-6, 6))])
        if r < 0.7:
            return f"({self.expr(d-1, vs)} {self.r.choice(['+', '-', '*', '%', '+', '-'])} {self.expr(d-1, vs)})"
        if r < 0.88:
            return f"{self.r.choice(F2)}({self.expr(d-1, vs)}, {self.expr(d-1, vs)})"
        if r < 0.95:
            return f"abs({self.expr(d-1, vs)})"
        return f"(if {self.cond(1, vs)} {{ {self.expr(d-1, vs)} }} else {{ {self.expr(d-1, vs)} }})"

    def atom(self, vs):
        a = self.r.choice(vs)
        b = self.r.choice(vs + [lit(self.r.randint(-6, 6))] * 3) if self.r.random() < 0.8 else self.expr(1, vs)
        return f"{a} {self.r.choice(['<', '<=', '>', '>=', '==', '!='])} {b}"

    def cond(self, d, vs):
        r = self.r.random()
        if d == 0 or r < 0.5:
            return self.atom(vs)
        if r < 0.7:
            return f"{self.cond(d-1, vs)} && {self.cond(d-1, vs)}"
        if r < 0.9:
            return f"{self.cond(d-1, vs)} || {self.cond(d-1, vs)}"
        return f"!({self.cond(d-1, vs)})"

    def block(self, depth, vs, ind):
        vs, out = list(vs), []
        for _ in range(self.r.randint(0, 3)):
            r = self.r.random()
            if r < 0.3:
                v = f"v{len(vs)}"
                out.append(f"{ind}let {v} = {self.expr(2, vs)}")
                vs.append(v)
            elif r < 0.45:
                out.append(f"{ind}require {self.cond(1, vs)} else Bad")
            elif r < 0.75 or depth == 0:
                out.append(f"{ind}if {self.cond(2, vs)} {{ return {self.expr(2, vs)} }}")
            elif r < 0.9:
                out.append(f"{ind}if {self.cond(2, vs)} {{\n{self.block(depth-1, vs, ind+'  ')}\n{ind}}}")
            else:
                out.append(f"{ind}if {self.cond(2, vs)} {{\n{self.block(depth-1, vs, ind+'  ')}\n{ind}}} else {{\n{self.block(depth-1, vs, ind+'  ')}\n{ind}}}")
        out.append(f"{ind}return {self.expr(2, vs)}")
        return "\n".join(out)


def campaign(soma, seed, n, workdir):
    g = Gen(seed)
    bodies = [g.block(2, ['n', 'k'], '    ') for _ in range(n)]
    req = f"requires n >= {lit(NL)} && n <= {lit(NH)} && k >= {lit(KL)} && k <= {lit(KH)}"
    # 1. the truth: every input
    out = ["cell C {", "  face {", "    signal scan() -> List", "  }"]
    for i, b in enumerate(bodies):
        out.append(f"  on _e{i}(n: Int, k: Int) {{\n{b}\n  }}")
    out.append("  on scan() {\n    let res = []")
    for i in range(n):
        out.append(f"""    let lo{i} = ()
    let hi{i} = ()
    let odd{i} = 0
    for n in range({lit(NL)}, {NH+1}) {{ for k in range({lit(KL)}, {KH+1}) {{
        let r = try {{ _e{i}(n, k) }}
        if r.error == () {{ if type_of(r.value) != "Int" {{ odd{i} += 1 }}
            if lo{i} == () || r.value < lo{i} {{ lo{i} = r.value }}
            if hi{i} == () || r.value > hi{i} {{ hi{i} = r.value }} }}
    }} }}
    res = push(res, [lo{i}, hi{i}, odd{i}])""")
    out.append("    return res\n  }\n}")
    scan = os.path.join(workdir, f"s{seed}.cell")
    open(scan, "w").write("\n".join(out))
    p = subprocess.run([soma, "run", scan, "scan"], capture_output=True, text=True, cwd=workdir)
    try:
        res = json.loads(p.stdout.strip().splitlines()[-1])
    except Exception:
        raise SystemExit(f"prove_fuzz: seed {seed}: the scan did not run:\n{p.stdout[-800:]}{p.stderr[-800:]}")
    # 2. what verify says of two false and two true clauses
    out, claims = ["cell C {", "  face {"], {}
    for i in range(n):
        lo, hi, odd = res[i]
        if lo is None or odd:
            continue
        for tag, cl in (('lo', f"result >= {lit(lo+1)}"), ('hi', f"result <= {lit(hi-1)}"), ('tl', f"result >= {lit(lo)}"), ('th', f"result <= {lit(hi)}")):
            out.append(f"    signal {tag}{i}(n: Int, k: Int) -> Int\n        {req}\n        ensures {cl}")
        claims[i] = (lo, hi)
    out.append("  }")
    for i in claims:
        for tag in ('lo', 'hi', 'tl', 'th'):
            out.append(f"  on {tag}{i}(n: Int, k: Int) {{\n{bodies[i]}\n  }}")
    out.append("}")
    ver = os.path.join(workdir, f"v{seed}.cell")
    open(ver, "w").write("\n".join(out))
    p = subprocess.run([soma, "verify", ver], capture_output=True, text=True, cwd=workdir)
    txt = p.stdout + p.stderr
    if claims and "ensures" not in txt:
        raise SystemExit(f"prove_fuzz: seed {seed}: verify printed no ensures line:\n{txt[-800:]}")
    tally = {"bodies": len(claims), "unsound": 0, "false_fail": 0, "proven": 0, "refuted": 0, "cases": []}
    for line in txt.splitlines():
        m = re.match(r"\s*✓ (lo|hi|tl|th)(\d+): ensures (.*?) — proven", line)
        if m and m.group(1) in ('lo', 'hi'):
            i = int(m.group(2))
            tally["unsound"] += 1
            tally["cases"].append({"kind": "unsound", "seed": seed, "claim": m.group(3), "actual": claims[i], "body": bodies[i]})
        elif m:
            tally["proven"] += 1
        if re.match(r"\s*✗ (lo|hi)\d+: ensures ", line):
            tally["refuted"] += 1
        m = re.match(r"\s*✗ (tl|th)(\d+): ensures (.*?) — (.*)", line)
        if m:
            i = int(m.group(2))
            tally["false_fail"] += 1
            tally["cases"].append({"kind": "false_fail", "seed": seed, "claim": m.group(3), "actual": claims[i], "said": m.group(4)[:160], "body": bodies[i]})
    return tally


def seeds_of(text):
    if "-" in text:
        a, b = text.split("-")
        return range(int(a), int(b) + 1)
    return [int(text)]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--soma", default=os.environ.get("SOMA", os.path.join(os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))), "compiler/target/release/soma")))
    ap.add_argument("--seeds", default="2-21")
    ap.add_argument("--bodies", type=int, default=40)
    ap.add_argument("--keep", action="store_true", help="write the programs in the current directory")
    ap.add_argument("--cases", type=int, default=5, help="how many cases to print in full")
    a = ap.parse_args()
    soma = os.path.abspath(a.soma)
    total = {"bodies": 0, "unsound": 0, "false_fail": 0, "proven": 0, "refuted": 0, "cases": []}
    with tempfile.TemporaryDirectory() as tmp:
        for s in seeds_of(a.seeds):
            t = campaign(soma, s, a.bodies, os.getcwd() if a.keep else tmp)
            for k in ("bodies", "unsound", "false_fail", "proven", "refuted"):
                total[k] += t[k]
            total["cases"] += t["cases"]
    # the false proofs first, then the shortest false failures: the ones to read
    total["cases"].sort(key=lambda c: (c["kind"] != "unsound", len(c["body"])))
    total["cases"] = total["cases"][:a.cases]
    print(json.dumps(total, indent=1, ensure_ascii=False))
    sys.exit(1 if total["unsound"] else 0)


if __name__ == "__main__":
    main()
