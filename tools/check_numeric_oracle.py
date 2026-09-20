#!/usr/bin/env python3
"""Compare Soma statistics with Python's exact fractions and Decimal sqrt.

Usage: SOMA=compiler/target/release/soma python3 tools/check_numeric_oracle.py
No third-party Python packages or network access are needed.
"""
import decimal
import fractions
import json
import math
import os
from pathlib import Path
import random
import subprocess
import tempfile


def mean(values):
    result = sum(map(fractions.Fraction, values)) / len(values)
    if all(isinstance(v, int) for v in values) and result.denominator == 1:
        return result.numerator
    try:
        return float(result)
    except OverflowError:
        return math.inf if result > 0 else -math.inf


def expected(values):
    ordered = sorted(values)
    m = len(values) // 2
    median = ordered[m] if len(values) % 2 else mean(ordered[m - 1:m + 1])
    exact = list(map(fractions.Fraction, values))
    center = sum(exact) / len(exact)
    squares = sum((x - center) ** 2 for x in exact)
    with decimal.localcontext() as context:
        context.prec = 2000
        def deviation(denominator):
            variance = squares / denominator
            exact_decimal = decimal.Decimal(variance.numerator) / decimal.Decimal(variance.denominator)
            return float(exact_decimal.sqrt())
        return [mean(values), median, deviation(len(values)), deviation(len(values) - 1)]


def main():
    rng = random.Random(284)
    vectors = [
        [1e308, 1e308], [-1e308, 1e308], [0.0, 2e-200],
        [0.0, 5e-324], [0.0, 1e-323], [10**400, 1.0, -10**400],
        [9007199254740993, -9007199254740992.0],
        [0.0, 9007199254740993, 9007199254740995],
        [0, 18014398509481986], [0, 18014398509481990],
    ]
    while len(vectors) < 320:
        values = []
        for _ in range(rng.randint(2, 6)):
            if rng.randrange(3) == 0:
                v = (1 << rng.randrange(1024)) + rng.randrange(16)
                values.append(v if rng.randrange(2) else -v)
            else:
                values.append(math.ldexp(rng.uniform(-1.0, 1.0), rng.randint(-1073, 1023)))
        vectors.append(values)

    source = '''cell Oracle {
 on measure(xs: List) {
  let a=avg(xs)
  let m=median(xs)
  let p=pstdev(xs)
  let s=stdev(xs)
  return [[type_of(a),to_string(a)],[type_of(m),to_string(m)],[type_of(p),to_string(p)],[type_of(s),to_string(s)]]
 }
 on run() {
'''
    source += '\n'.join('  print(to_json(measure(' + repr(v) + ')))' for v in vectors)
    source += '\n }\n}\n'
    binary = os.environ.get('SOMA', 'soma')
    if os.sep in binary:
        binary = str(Path(binary).resolve())
    with tempfile.TemporaryDirectory(prefix='soma-numeric-oracle-') as directory:
        path = Path(directory) / 'app.cell'
        path.write_text(source)
        result = subprocess.run([binary, 'run', str(path)], cwd=directory,
                                capture_output=True, text=True, timeout=90)
        if result.returncode:
            raise AssertionError(result.stdout + result.stderr)
    rows = [json.loads(line) for line in result.stdout.splitlines() if line.startswith('[')]
    assert len(rows) == len(vectors), result.stdout
    for i, (values, actual) in enumerate(zip(vectors, rows)):
        assert len(actual) == 4, (i, actual)
        for operation, (kind, text), reference in zip(('avg', 'median', 'pstdev', 'stdev'), actual, expected(values)):
            want_kind = 'Int' if isinstance(reference, int) else 'Float'
            got = int(text) if kind == 'Int' else float(text)
            assert kind == want_kind and got == reference, (i, operation, values, kind, text, want_kind, reference)
    print(f'Numeric oracle: {len(vectors)} vectors, {4 * len(vectors)} exact comparisons passed')


if __name__ == '__main__':
    main()
