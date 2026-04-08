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

def first_triangle_with_divisors(d):
    n = 1
    while True:
        t = n * (n + 1) // 2
        if tau(t) > d: return t
        n += 1

def workload():
    first_triangle_with_divisors(500)

inner(workload)
