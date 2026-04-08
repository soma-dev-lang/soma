from _inner import inner
from numba import njit

@njit(cache=True)
def is_prime(n):
    if n < 2: return 0
    if n < 4: return 1
    if n % 2 == 0: return 0
    i = 3
    while i * i <= n:
        if n % i == 0: return 0
        i += 2
    return 1

def pi(n):
    return sum(is_prime(i) for i in range(2, n + 1))

def workload():
    # Same workload as sieve.cell run()
    for n in (10, 100, 1000, 10000, 100000, 1000000):
        pi(n)

def warmup():
    try: is_prime(2)
    except Exception: pass

inner(workload, warmup=warmup)