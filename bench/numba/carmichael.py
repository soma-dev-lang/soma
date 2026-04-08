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
def is_carmichael(n):
    if n < 2: return 0
    if is_prime(n) == 1: return 0
    m, p = n, 2
    while p * p <= m:
        if m % p == 0:
            m //= p
            if m % p == 0: return 0
            if (n - 1) % (p - 1) != 0: return 0
        else:
            p += 1
    if m > 1:
        if (n - 1) % (m - 1) != 0: return 0
    return 1

@njit(cache=True)
def count_under(limit):
    count = 0
    n = 3
    while n < limit:
        if is_carmichael(n) == 1: count += 1
        n += 2
    return count

def workload():
    # Same as carmichael.cell run()
    for n in (561, 1105, 1729, 2465):
        is_carmichael(n)
    count_under(10000)

def warmup():
    try: is_prime(2)
    except Exception: pass
    try: is_carmichael(2)
    except Exception: pass
    try: count_under(2)
    except Exception: pass

inner(workload, warmup=warmup)