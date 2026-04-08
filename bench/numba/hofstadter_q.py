from _inner import inner
from numba import njit

import sys
sys.setrecursionlimit(20000)

@njit(cache=True)
def hq_table(target):
    """Build the Q table iteratively (matches the cell's approach but
    with a Python list instead of a packed BigInt)."""
    if target <= 2: return 1
    q = [0, 1, 1] + [0] * (target - 2)
    for k in range(3, target + 1):
        q1 = q[k - 1]
        q2 = q[k - 2]
        q[k] = q[k - q1] + q[k - q2]
    return q[target]

def workload():
    for n in (1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 20, 100, 1000):
        hq_table(n)
    hq_table(10000)
    hq_table(1000000)

def warmup():
    try: hq_table(2)
    except Exception: pass

inner(workload, warmup=warmup)