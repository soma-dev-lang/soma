from _inner import inner
def tau(n):
    if n < 1: return 0
    count = 1
    m, p = n, 2
    while p * p <= m:
        if m % p == 0:
            e = 0
            while m % p == 0:
                m //= p
                e += 1
            count *= e + 1
        p += 1
    if m > 1: count *= 2
    return count

def sigma(n):
    if n < 1: return 0
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
    return total

def workload():
    # Same workload as divisors.cell run()
    for n in (1, 12, 60, 720, 7560):
        tau(n)
    for n in (1, 6, 28, 496, 8128, 720):
        sigma(n)

inner(workload)
