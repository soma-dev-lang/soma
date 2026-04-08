from _inner import inner
from numba import njit

@njit(cache=True)
def mu(n):
    if n == 1: return 1
    count = 0
    m = n
    p = 2
    while p * p <= m:
        if m % p == 0:
            m //= p
            count += 1
            if m % p == 0: return 0
        p += 1
    if m > 1: count += 1
    return 1 if count % 2 == 0 else -1

def mertens(n):
    return sum(mu(k) for k in range(1, n + 1))

def workload():
    # Same workload as mobius.cell run()
    for n in (1, 2, 3, 4, 6, 30, 105, 210):
        mu(n)
    mertens(100)
    mertens(1000)
    mertens(10000)

def warmup():
    try: mu(2)
    except Exception: pass

inner(workload, warmup=warmup)