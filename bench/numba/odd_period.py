from _inner import inner
from math import isqrt

def period(n):
    a0 = isqrt(n)
    if a0 * a0 == n: return 0
    m, d, a = 0, 1, a0
    p = 0
    while True:
        m = d * a - m
        d = (n - m * m) // d
        a = (a0 + m) // d
        p += 1
        if a == 2 * a0: return p

def count_odd_periods(limit):
    return sum(1 for n in range(2, limit + 1) if (p := period(n)) > 0 and p % 2 == 1)

def workload():
    count_odd_periods(10000)

inner(workload)
