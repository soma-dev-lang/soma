from _inner import inner
from math import floor

def series(j, n):
    s = 0.0
    for k in range(n + 1):
        denom = 8 * k + j
        r = pow(16, n - k, denom)
        s += r / denom
        s -= floor(s)
    p16 = 1.0 / 16.0
    for kk in range(n + 1, n + 31):
        s += p16 / (8 * kk + j)
        p16 /= 16.0
    return s

def bbp_hex(n):
    m = n - 1
    s = 4.0*series(1, m) - 2.0*series(4, m) - series(5, m) - series(6, m)
    s -= floor(s)
    if s < 0: s += 1
    return floor(s * 16)

def workload():
    for n in range(1, 13):
        bbp_hex(n)
    for n in (20, 30, 40, 50):
        bbp_hex(n)
    bbp_hex(100000)

inner(workload)
