#!/usr/bin/env python3
"""Does `[native]` evaluate arguments in the order written, each once?

`tools/diff_fuzz.py` compares VALUES; two arguments evaluated in the wrong
order, or twice, give the same value. Here every argument is a sibling
handler that prints a letter (`a()` prints A and returns 5, …), so the order
and the number of evaluations are on stdout:

    bit_test(a(), b())        interpreted: AB…   native must print AB… too

One native handler and one interpreted handler per shape; a shape whose two
outputs differ is reported. The native-only primitives (buf_set, hm_set,
sb_push…) have no interpreted twin: their expected trace is written here.

    python3 tools/harness/native_order.py [--soma PATH] [--keep]

Prints {"shapes", "differing", "cases"}; exit 1 when a shape differs.
Needs cargo on PATH (native handlers are compiled). Standard library only.
"""
import argparse, json, os, subprocess, sys, tempfile

G = "shl(1, 70)"  # past 64 bits: the handler runs in BigInt mode
SHAPES = {
    # ints, i64 mode
    'idiv': 'return idiv(a(), b())', 'gcd': 'return gcd(a(), b())', 'pow_mod': 'return pow_mod(a(), b(), c())',
    'min': 'return min(a(), b())', 'max': 'return max(a(), b())', 'band': 'return band(a(), b())', 'bor': 'return bor(a(), b())',
    'bxor': 'return bxor(a(), b())', 'shl': 'return shl(a(), b())', 'shr': 'return shr(a(), b())', 'bit_next': 'return bit_next(a(), b())',
    'bit_test': 'return bit_test(a(), b())', 'bit_set': 'return bit_set(a(), b())', 'bit_clr': 'return bit_clr(a(), b())',
    'add': 'return a() + b()', 'sub': 'return a() - b()', 'mul': 'return a() * b()', 'mod': 'return a() % b()',
    'lt': 'if a() < b() { return 1 }\n return 0', 'eq': 'if a() == b() { return 1 }\n return 0',
    'and': 'if a() > 9 && b() > 0 { return 1 }\n return 0', 'or': 'if a() > 0 || b() > 0 { return 1 }\n return 0',
    'abs': 'return abs(a())', 'sqrt_int': 'return sqrt_int(a())', 'bit_len': 'return bit_len(a())', 'bnot': 'return bnot(a())',
    'nested': 'return max(a(), min(b(), c()))', 'neg': 'return 0 - a() - b()', 'sibling': 'return two(a(), b())',
    'siblings': 'return two(b(), a()) + two(c(), a())',
    'compound': 'let v = 1\n v += a()\n v *= b()\n v -= c()\n return v',
    'while': 'let v = 0\n while v < b() { v += a() }\n return v', 'for': 'let v = 0\n for i in range(a() - 5, b()) { v += 1 }\n return v',
    # BigInt mode
    'big_shl': f'let g = {G}\n return shl(a() + g, b())', 'big_shr': f'let g = {G}\n return shr(a() + g, b())',
    'big_bit_set': f'let g = {G}\n return bit_set(a() + g, b())', 'big_bit_clr': f'let g = {G}\n return bit_clr(a() + g, b())',
    'big_bit_test': f'let g = {G}\n return bit_test(a() + g, b())', 'big_bit_next': f'let g = {G}\n return bit_next(a() + g, b())',
    'big_gcd': f'let g = {G}\n return gcd(a() + g, b() + g)', 'big_idiv': f'let g = {G}\n return idiv(a() + g, b() + g)',
    'big_mod': f'let g = {G}\n return (a() + g) % (b() + g)', 'big_mul': f'let g = {G}\n return (a() + g) * (b() + g)',
    'big_band': f'let g = {G}\n return band(a() + g, b() + g)', 'big_bxor': f'let g = {G}\n return bxor(a() + g, b())',
    'big_min': f'let g = {G}\n return min(a() + g, b() + g)', 'big_max': f'let g = {G}\n return max(b() + g, a())',
    'big_pow_mod': f'let g = {G}\n return pow_mod(a() + g, b(), c())', 'big_pow_mod_m': f'let g = {G}\n return pow_mod(a(), b(), c() + g)',
    'big_abs': f'let g = {G}\n return abs(a() - g)', 'big_sqrt_int': f'let g = {G}\n return sqrt_int(a() + g)',
    'big_bit_len': f'let g = {G}\n return bit_len(a() + g)', 'big_bnot': f'let g = {G}\n return bnot(a() + g)',
    'big_lt': f'let g = {G}\n if a() + g < b() + g {{ return 1 }}\n return 0', 'big_eq': f'let g = {G}\n if a() + g == b() + g {{ return 1 }}\n return 0',
    'big_sub': f'let g = {G}\n return (a() - g) - (b() - c())', 'big_to_string': f'let g = {G}\n return str_len(to_string(a() + g))',
    # floats and strings
    'f_min': 'return to_int(min(fa(), fb()))', 'f_max': 'return to_int(max(fa(), fb()))', 'f_abs': 'return to_int(abs(fa()))',
    'f_pow': 'return to_int(pow(fa(), fb()))', 'f_round': 'return round(fa())', 'f_floor_ceil': 'return floor(fa()) + ceil(fb())',
    'f_mixed': 'return to_int(min(fa(), b()))', 'f_mixed2': 'return to_int(max(a(), fb()))', 'f_lt': 'if fa() < b() { return 1 }\n return 0',
    'f_mod': 'return to_int(fa() % fb())', 'f_div': 'return to_int(fa() / fb())', 'i_div': 'return to_int(a() / b())',
    'atan2': 'return to_int(atan2(fa(), fb()))',
    'str_eq': 'if str_eq(sa(), sb()) { return 1 }\n return 0', 'str_len': 'return str_len(sa())', 'str_cat': 'return str_len(sa() + sb())',
    'str_at': 'return str_at(sa(), b())', 'to_string': 'return str_len(to_string(a()) + to_string(fb()))',
    'f_unary': 'return to_int(sqrt(fa()) + log(fb()) + exp(fa()) + sin(fb()) + cos(fa()) + tan(fb()) + atan(fa()) + to_float(b()))',
    'f_arc': 'return to_int(10.0 * (asin(fa() - fb() - 0.5) + acos(fb() - fa() + 1.5)))',
    'len': 'return len(sa()) + b()',
    'regex_count': 'return regex_count(sa(), "l")', 'regex_match': 'return regex_match(sa(), "l") + b()', 'regex_replace': 'return str_len(regex_replace(sa(), "l", sb()))',
}
# two arguments that both fail: the error is the FIRST one's (`z()` is 0)
SHAPES.update({
    'err_bit_clr': 'return bit_clr(5 % z(), z() - 1)', 'err_bit_set': 'return bit_set(idiv(5, z()), z() - 1)',
    'err_bit_test': 'return bit_test(5 % z(), z() - 1)', 'err_shl': 'return shl(idiv(1, z()), z() - 1)',
    'err_idiv': 'return idiv(shl(1, z() - 1), z())', 'err_mod': 'return shl(1, z() - 1) % z()',
    'err_pow_mod': 'return pow_mod(5 % z(), z() - 1, z())', 'err_pow_mod2': 'return pow_mod(5, shl(1, z() - 1), z())',
    'err_gcd': 'return gcd(idiv(1, z()), shl(1, z() - 1))', 'err_min': 'return min(shl(1, z() - 1), idiv(1, z()))',
    'err_add': 'return idiv(1, z()) + shl(1, z() - 1)', 'err_sibling': 'return two(shl(1, z() - 1), idiv(1, z()))',
})
# native-only primitives: (body, the trace and result it must print)
NATIVE_ONLY = {
    'buf_set': ('let u = buffer(10)\n buf_set(u, a(), b())\n return buf_get(u, a())', 'ABA3'),
    'buf_set_f': ('let u = buffer_f(10)\n buf_set_f(u, a(), fb())\n return to_int(buf_get_f(u, a()))', 'AGA1'),
    'hm_set': ('let h = hashmap()\n hm_set(h, a(), b())\n hm_inc(h, a())\n return hm_get(h, a()) + hm_len(h)', 'ABAA5'),
    'sb_push': ('let t = strbuf()\n sb_push_int(t, a())\n sb_push(t, sa())\n sb_push_char(t, b() + 60)\n return sb_len(t)', 'ASB7'),
    'hm_has': ('let h = hashmap()\n hm_set(h, a(), b())\n if hm_has(h, b()) { return 1 }\n return 0', 'ABB0'),
    'sb_finish': ('let t = strbuf()\n sb_push(t, sa())\n return str_len(sb_finish(t)) + b()', 'SB8'),
    'big_buf_set': (f'let g = {G}\n let u = buffer(10)\n buf_set(u, a(), b())\n return buf_get(u, a()) + bit_len(g)', 'ABA74'),
}
# builtins of the native vocabulary with no shape, and why: anything else in
# `ALLOWED_BUILTINS` (compiler/src/checker/native.rs) must appear in a shape —
# a builtin added there without one makes this tool fail
EXEMPT = {
    "pi": "no argument", "random": "no argument, and no two runs agree",
    "map": "takes a lambda: what it evaluates is the list before it", "filter": "takes a lambda", "reduce": "takes a lambda", "fold": "takes a lambda",
    "hashmap": "no argument", "strbuf": "no argument",
    "read_file": "reads the world", "read_stdin": "reads the world",
    # a finding of this tool: the checker lists `nth`, the native compiler has no such call
    # ("unknown function call: nth") — a handler using it is refused at `check`, with that message
    "nth": "in the checker's list, not in the native compiler",
}


def vocabulary():
    """The builtins a [native] handler may call, as the checker lists them."""
    import re
    root = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
    src = open(os.path.join(root, "compiler", "src", "checker", "native.rs"), encoding="utf-8").read()
    # (comments out first: one of them writes `buf[i];`)
    block = "\n".join(l.split("//")[0] for l in src[src.index("const ALLOWED_BUILTINS"):].splitlines())
    block = block[:block.index("];")]
    return re.findall(r'"([a-z_0-9]+)"', block)


def uncovered():
    import re
    bodies = " ".join(SHAPES.values()) + " " + " ".join(b for b, _ in NATIVE_ONLY.values()) + ' write_str("x")'
    return [n for n in vocabulary() if n not in EXEMPT and not re.search(r"\b" + n + r"\(", bodies)]


PRINTERS = [('a', 'Int', 'A', '5'), ('b', 'Int', 'B', '3'), ('c', 'Int', 'C', '7'), ('sa', 'String', 'S', '"hello"'),
            ('sb', 'String', 'T', '"l"'), ('fa', 'Float', 'F', '2.5'), ('fb', 'Float', 'G', '1.5'), ('z', 'Int', 'Z', '0')]


def program():
    src = ["cell C {", "  face {"] + [f"    signal {n}() -> {t}" for n, t, _, _ in PRINTERS] + ["    signal two(x: Int, y: Int) -> Int"]
    for k in SHAPES:
        src += [f"    signal n_{k}() -> Int", f"    signal i_{k}() -> Int"]
    src += [f"    signal n_{k}() -> Int" for k in NATIVE_ONLY] + ["  }"]
    src += [f'  on {n}() [native] {{ write_str("{letter}")\n return {v} }}' for n, _, letter, v in PRINTERS]
    src.append("  on two(x: Int, y: Int) [native] { return x * 10 + y }")
    for k, body in SHAPES.items():
        src += [f"  on n_{k}() [native] {{ {body} }}", f"  on i_{k}() {{ {body} }}"]
    src += [f"  on n_{k}() [native] {{ {body} }}" for k, (body, _) in NATIVE_ONLY.items()] + ["}"]
    return "\n".join(src)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--soma", default=os.environ.get("SOMA", os.path.join(os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))), "compiler/target/release/soma")))
    ap.add_argument("--keep", action="store_true", help="write order.cell in the current directory")
    a = ap.parse_args()
    soma = os.path.abspath(a.soma)
    with tempfile.TemporaryDirectory() as tmp:
        d = os.getcwd() if a.keep else tmp
        open(os.path.join(d, "order.cell"), "w").write(program())
        p = subprocess.run([soma, "check", "order.cell"], capture_output=True, text=True, cwd=d)
        errors = [l for l in (p.stdout + p.stderr).splitlines() if l.startswith("error")]
        if errors:
            raise SystemExit("native_order: order.cell does not check:\n" + "\n".join(e[:300] for e in errors[:5]))

        def run(h):
            p = subprocess.run([soma, "run", "order.cell", h], capture_output=True, text=True, cwd=d)
            err = " ".join(l for l in p.stderr.splitlines() if l.startswith("error"))
            return (p.stdout.strip().replace("\n", "") + (" " + err if err else ""))[:120]

        cases = []
        for k in SHAPES:
            want, got = run(f"i_{k}"), run(f"n_{k}")
            if want != got:
                cases.append({"shape": k, "body": SHAPES[k], "interpreted": want, "native": got})
        for k, (body, want) in NATIVE_ONLY.items():
            got = run(f"n_{k}")
            if want != got:
                cases.append({"shape": k, "body": body, "expected": want, "native": got})
    missing = uncovered()
    print(json.dumps({"shapes": len(SHAPES) + len(NATIVE_ONLY), "differing": len(cases), "uncovered": missing, "cases": cases}, indent=1, ensure_ascii=False))
    sys.exit(1 if cases or missing else 0)


if __name__ == "__main__":
    main()
