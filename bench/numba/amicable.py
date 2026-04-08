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
def amicable_sum(limit):
    s = 0
    for i in range(2, limit + 1):
        j = aliquot(i)
        if j > i and j <= limit and aliquot(j) == i:
            s += i + j
    return s

def workload():
    # Same as amicable.cell run()
    aliquot(220); aliquot(284); aliquot(1184)
    amicable_sum(10000)

def warmup():
    try: aliquot(2)
    except Exception: pass
    try: amicable_sum(2)
    except Exception: pass

inner(workload, warmup=warmup)