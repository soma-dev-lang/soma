from _inner import inner
from numba import njit

@njit(cache=True)
def order(a, p):
    v = a % p
    k, cur = 1, v
    while cur != 1:
        cur = (cur * v) % p
        k += 1
        if k > p: return -1
    return k

@njit(cache=True)
def smallest_primitive_root(p):
    g = 2
    while g < p:
        if order(g, p) == p - 1: return g
        g += 1
    return -1

def workload():
    # Same as cyclotomic.cell run()
    order(3, 7); order(2, 11); order(5, 13)
    for p in (7, 11, 23, 41):
        smallest_primitive_root(p)

def warmup():
    try: order(2, 2)
    except Exception: pass
    try: smallest_primitive_root(2)
    except Exception: pass

inner(workload, warmup=warmup)