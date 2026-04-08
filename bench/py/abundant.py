from _inner import inner

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

def is_abundant(n):
    return aliquot(n) > n

def pe23_under(limit):
    abundants = [n for n in range(12, limit + 1) if is_abundant(n)]
    abundant_set = set(abundants)
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
    pe23_under(50)
    pe23_under(1000)

inner(workload)
