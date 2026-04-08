from _inner import inner
from math import isqrt

def period(n):
    a0 = isqrt(n)
    if a0 * a0 == n: return 0
    m, d, a = 0, 1, a0
    length = 0
    stop = 2 * a0
    while True:
        m = d * a - m
        d = (n - m * m) // d
        a = (a0 + m) // d
        length += 1
        if a == stop: return length

def longest_period(limit):
    best_n = 1
    best_p = 0
    for i in range(2, limit):
        p = period(i)
        if p > best_p:
            best_p = p
            best_n = i
    return (best_n, best_p)

def workload():
    # Match cell's headline: longest period for n < 100,000
    longest_period(100000)

inner(workload)
