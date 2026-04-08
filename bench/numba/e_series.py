from _inner import inner
from numba import njit

@njit(cache=True)
def compute_e():
    term = 1.0
    s = 1.0
    for k in range(1, 100):
        term /= k
        s += term
        if term < 1e-16: return s
    return s

def workload():
    compute_e()

def warmup():
    try: compute_e()
    except Exception: pass

inner(workload, warmup=warmup)