from _inner import inner
from numba import njit


@njit(cache=True)
def aliquot(n):
    if n < 2: return 0
    total = 1
    m, p = n, 2
    while p * p <= m:
        if m % p == 0:
            pe = 1
            while m % p == 0:
                m //= p
                pe *= p
            total *= (p * pe - 1) // (p - 1)
        p += 1
    if m > 1: total *= m + 1
    return total - n

@njit(cache=True)
def is_abundant(n):
    return aliquot(n) > n

def pe23():
    limit = 28123
    abundant_set = set(n for n in range(12, limit + 1) if is_abundant(n))
    abundants = sorted(abundant_set)
    total = 0
    for n in range(1, limit + 1):
        found = False
        for a in abundants:
            if a > n // 2: break
            if (n - a) in abundant_set:
                found = True
                break
        if not found:
            total += n
    return total

def workload():
    pe23()

def warmup():
    try: aliquot(2)
    except Exception: pass
    try: is_abundant(2)
    except Exception: pass

inner(workload, warmup=warmup)