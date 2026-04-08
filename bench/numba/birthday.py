from _inner import inner
from numba import njit


@njit(cache=True)
def prob(k, n):
    p = 1.0
    for i in range(1, k):
        p *= (1.0 - i / n)
    return 1.0 - p

@njit(cache=True)
def smallest_k(threshold, n):
    for k in range(1, n + 1):
        if prob(k, n) >= threshold: return k
    return -1

def workload():
    prob(23, 365)
    smallest_k(0.5, 365)
    smallest_k(0.99, 365)

def warmup():
    try: prob(2, 2)
    except Exception: pass
    try: smallest_k(2, 2)
    except Exception: pass

inner(workload, warmup=warmup)