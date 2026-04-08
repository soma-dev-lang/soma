from _inner import inner
from numba import njit

@njit(cache=True)
def is_prime(n):
    if n < 2: return False
    if n < 4: return True
    if n % 2 == 0: return False
    i = 3
    while i * i <= n:
        if n % i == 0: return False
        i += 2
    return True

@njit(cache=True)
def twin_count(limit):
    count = 0
    p = 3
    while p + 2 <= limit:
        if is_prime(p) and is_prime(p + 2):
            count += 1
        p += 2
    return count

def workload():
    # Same workload as twin_primes.cell run()
    for n in (100, 1000, 10000, 100000, 1000000):
        twin_count(n)

def warmup():
    try: is_prime(2)
    except Exception: pass
    try: twin_count(2)
    except Exception: pass

inner(workload, warmup=warmup)