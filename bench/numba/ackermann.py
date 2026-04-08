from _inner import inner
from numba import njit

import sys
sys.setrecursionlimit(20000)

@njit(cache=True)
def ackermann(m, n):
    if m == 0: return n + 1
    if n == 0: return ackermann(m - 1, 1)
    return ackermann(m - 1, ackermann(m, n - 1))

def workload():
    for (m, n) in [(0,0),(0,5),(1,0),(1,5),(2,0),(2,5),(3,0),(3,5),(3,8),(3,10)]:
        ackermann(m, n)

def warmup():
    try: ackermann(2, 2)
    except Exception: pass

inner(workload, warmup=warmup)