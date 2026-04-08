from _inner import inner

def miller_rabin(n, a):
    if n % a == 0: return n == a
    d, s = n - 1, 0
    while d % 2 == 0:
        d //= 2
        s += 1
    x = pow(a, d, n)
    if x == 1 or x == n - 1: return True
    for _ in range(s - 1):
        x = (x * x) % n
        if x == n - 1: return True
    return False

def is_prime(n):
    if n < 2: return False
    for a in (2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37):
        if a >= n: return True
        if not miller_rabin(n, a): return False
    return True

def workload():
    for p in (521, 607, 67, 1279, 2203, 2281, 3217, 4253):
        is_prime((1 << p) - 1)

inner(workload)
