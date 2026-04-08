from _inner import inner
from math import isqrt

def cf_sqrt(n, terms):
    a0 = isqrt(n)
    if a0 * a0 == n: return [a0]
    out = [a0]
    m, d, a = 0, 1, a0
    for _ in range(terms):
        m = d * a - m
        d = (n - m * m) // d
        a = (a0 + m) // d
        out.append(a)
    return out

def workload():
    for (n, t) in [(2, 30), (3, 30), (5, 30), (7, 30), (23, 30), (97, 30)]:
        cf_sqrt(n, t)

inner(workload)
