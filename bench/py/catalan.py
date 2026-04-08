from _inner import inner
from math import comb

def catalan(n):
    return comb(2 * n, n) // (n + 1)

def workload():
    for n in (0, 1, 5, 10, 15, 20, 30, 50, 100):
        catalan(n)

inner(workload)
