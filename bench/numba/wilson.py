# Numba's int is fixed-width int64. This cell needs arbitrary precision
# (values exceed 2^63), so we use plain Python int — Numba would
# silently overflow and return garbage in microseconds.
from _inner import inner
def factorial_mod(n):
    f = 1
    for i in range(2, n):
        f = (f * i) % n
    return f

def is_prime(n):
    if n < 2: return 0
    if n == 2: return 1
    return 1 if factorial_mod(n) == n - 1 else 0

def count_primes(limit):
    return sum(is_prime(n) for n in range(2, limit + 1))

def workload():
    # Same as wilson.cell run()
    count_primes(100)
    count_primes(500)
    count_primes(1000)

inner(workload)
