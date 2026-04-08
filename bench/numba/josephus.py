from _inner import inner
from numba import njit

@njit(cache=True)
def josephus(n, k):
    j = 0
    for i in range(2, n + 1):
        j = (j + k) % i
    return j + 1

def workload():
    for (n, k) in [(7,3),(10,2),(14,2),(40,3),(100,7),(1000,1),(1000000,5)]:
        josephus(n, k)

def warmup():
    try: josephus(2, 2)
    except Exception: pass

inner(workload, warmup=warmup)