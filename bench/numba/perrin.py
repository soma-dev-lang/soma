from _inner import inner
from numba import njit

@njit(cache=True)
def perrin(n):
    if n == 0: return 3
    if n == 1: return 0
    if n == 2: return 2
    a, b, c = 3, 0, 2
    for _ in range(3, n + 1):
        a, b, c = b, c, a + b
    return c

@njit(cache=True)
def perrin_test(n):
    return 1 if perrin(n) % n == 0 else 0

def count_primes_via_perrin(limit):
    return sum(perrin_test(n) for n in range(2, limit + 1))

def workload():
    # Same as perrin.cell run()
    for n in (0, 5, 10, 20):
        perrin(n)
    count_primes_via_perrin(2000)

def warmup():
    try: perrin(2)
    except Exception: pass
    try: perrin_test(2)
    except Exception: pass

inner(workload, warmup=warmup)