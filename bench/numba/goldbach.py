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
def goldbach_smallest(n):
    for p in range(2, n + 1):
        if is_prime(p) and is_prime(n - p):
            return p
    return -1

@njit(cache=True)
def verify_up_to(limit):
    count = 0
    n = 4
    while n <= limit:
        if goldbach_smallest(n) > 0:
            count += 1
        n += 2
    return count

def workload():
    # Match cell's headline: verify Goldbach for all even n ≤ 1,000,000
    verify_up_to(1_000_000)

def warmup():
    try: is_prime(2)
    except Exception: pass
    try: goldbach_smallest(2)
    except Exception: pass
    try: verify_up_to(2)
    except Exception: pass

inner(workload, warmup=warmup)