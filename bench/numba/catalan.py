# Catalan numbers grow as ~4^n / sqrt(n) — C_30000 is a 6000-digit
# integer. Numba's int is fixed-width int64 (max ~19 digits) so @njit
# would compute garbage for any n > 30. Plain Python int is the only
# correct path.
import sys
sys.set_int_max_str_digits(100000)

from _inner import inner


def catalan(n):
    c = 1
    for i in range(n):
        c = c * (4 * i + 2) // (i + 2)
    return c


def catalan_digits(n):
    return len(str(catalan(n)))


def workload():
    # Match cell's full run() workload, including digit counts up to C_30000
    for n in (0, 1, 2, 5, 10, 20, 50):
        catalan(n)
    for n in (100, 500, 1000, 5000, 10000, 30000):
        catalan_digits(n)


inner(workload)
