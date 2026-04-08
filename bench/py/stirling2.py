from _inner import inner
from math import comb, factorial

def s2(n, k):
    if k == 0: return 1 if n == 0 else 0
    total = 0
    for j in range(k + 1):
        sign = 1 if j % 2 == 0 else -1
        total += sign * comb(k, j) * (k - j) ** n
    return total // factorial(k)

def s2_rec(n, k):
    if k == 0: return 1 if n == 0 else 0
    if n == 0 or k > n: return 0
    if k == n or k == 1: return 1
    return k * s2_rec(n - 1, k) + s2_rec(n - 1, k - 1)

def workload():
    # Same as stirling2.cell run()
    s2(4, 2); s2(5, 3); s2(6, 3); s2(10, 3); s2(20, 5)
    s2_rec(8, 4)

inner(workload)
