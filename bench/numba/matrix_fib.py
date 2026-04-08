# Matrix-exponentiation Fibonacci grows as ~F_n which exceeds int64 at
# n > 93. Numba's i64 silently overflows. Plain Python int (which has
# arbitrary precision) is the only correct path.
import sys
sys.set_int_max_str_digits(10000000)

from _inner import inner


def fib_matrix(n):
    if n == 0: return 0
    if n == 1: return 1
    ra, rb, rc, rd = 1, 0, 0, 1
    ba, bb, bc, bd = 1, 1, 1, 0
    m = n
    while m > 0:
        if m % 2 == 1:
            na = ra*ba + rb*bc
            nb = ra*bb + rb*bd
            nc = rc*ba + rd*bc
            nd = rc*bb + rd*bd
            ra, rb, rc, rd = na, nb, nc, nd
        sa = ba*ba + bb*bc
        sb = ba*bb + bb*bd
        sc = bc*ba + bd*bc
        sd = bc*bb + bd*bd
        ba, bb, bc, bd = sa, sb, sc, sd
        m //= 2
    return rb


def workload():
    # Match cell's full run() workload
    for n in (0, 1, 2, 10, 20, 50, 100):
        fib_matrix(n)
    # Larger digit-count checks
    for n in (1000, 10000, 100000):
        len(str(fib_matrix(n)))
    # Headline
    fib_matrix(1_000_000)


inner(workload)
