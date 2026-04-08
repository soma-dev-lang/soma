from _inner import inner

from math import isqrt

def workload():
    for n in (0, 1, 4, 100, 99999999, 1 << 100, 1 << 200, 1 << 500, 1 << 1000, 1 << 2000):
        isqrt(n)

inner(workload)
