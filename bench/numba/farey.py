from _inner import inner
from numba import njit


@njit(cache=True)
def phi(n):
    if n <= 1: return n
    result = n
    m, p = n, 2
    while p * p <= m:
        if m % p == 0:
            while m % p == 0: m //= p
            result -= result // p
        p += 1
    if m > 1: result -= result // m
    return result

def farey_size(n):
    return 1 + sum(phi(k) for k in range(1, n + 1))

def workload():
    for n in (8, 100, 1000, 10000):
        farey_size(n)

def warmup():
    try: phi(2)
    except Exception: pass

inner(workload, warmup=warmup)