# Numba's int is fixed-width int64. This cell needs arbitrary precision
# (values exceed 2^63), so we use plain Python int — Numba would
# silently overflow and return garbage in microseconds.
from _inner import inner

def is_prime(n):
    if n < 2: return 0
    if n < 4: return 1
    if n % 2 == 0: return 0
    i = 3
    while i * i <= n:
        if n % i == 0: return 0
        i += 2
    return 1

def lucas_lehmer(p):
    if p == 2: return 1
    m = (1 << p) - 1
    s = 4
    for _ in range(p - 2):
        s = (s * s - 2) % m
    return 1 if s == 0 else 0

def count_mp(p_max):
    return sum(lucas_lehmer(p) for p in range(2, p_max + 1) if is_prime(p))

def workload():
    count_mp(31)
    count_mp(100)

inner(workload)
