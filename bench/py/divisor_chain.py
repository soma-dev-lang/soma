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

def chain_length(n, cap):
    cur = n
    for i in range(cap):
        nxt = aliquot(cur)
        if nxt == 0 or nxt == cur: return i + 1
        cur = nxt
    return cap

def workload():
    chain_length(95, 50)
    chain_length(220, 50)
    chain_length(12496, 50)

inner(workload)
