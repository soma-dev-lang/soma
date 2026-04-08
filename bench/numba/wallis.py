from _inner import inner
from numba import njit


@njit(cache=True)
def wallis_pi(n):
    p = 1.0
    for k in range(1, n + 1):
        kk = k * k
        p *= (4.0 * kk) / (4.0 * kk - 1.0)
    return 2.0 * p

def workload():
    wallis_pi(1_000_000)

def warmup():
    try: wallis_pi(2)
    except Exception: pass

inner(workload, warmup=warmup)