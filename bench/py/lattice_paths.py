from _inner import inner
from math import comb

def workload():
    for n in (2, 3, 10, 20, 30, 50):
        comb(2 * n, n)

inner(workload)
