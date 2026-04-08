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

@njit(cache=True)
def count_under(limit):
    count = 0
    p = 2
    while p <= limit:
        if is_prime(p) == 1 and is_prime(2 * p + 1) == 1:
            count += 1
        p = 3 if p == 2 else p + 2
    return count

def workload():
    # Same as sophie_germain.cell run()
    for n in (100, 1000, 10000, 100000):
        count_under(n)

def warmup():
    try: is_prime(2)
    except Exception: pass
    try: count_under(2)
    except Exception: pass

inner(workload, warmup=warmup)