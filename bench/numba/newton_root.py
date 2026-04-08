from _inner import inner
from numba import njit

@njit(cache=True)
def nth_root(a, m):
    x = a
    for _ in range(100):
        prev = x
        x = ((m - 1) * x + a / (x ** (m - 1))) / m
        if abs(x - prev) < 1e-12: return x
    return x

def workload():
    # Same as newton_root.cell run()
    nth_root(2.0, 2)
    nth_root(2.0, 3)
    nth_root(729.0, 6)
    nth_root(2.0, 10)

def warmup():
    try: nth_root(2, 2)
    except Exception: pass

inner(workload, warmup=warmup)