from _inner import inner
from numba import njit

@njit(cache=True)
def egcd(a, b):
    if b == 0: return (a, 1, 0)
    g, x1, y1 = egcd(b, a % b)
    return (g, y1, x1 - (a // b) * y1)

@njit(cache=True)
def mod_inv(a, m):
    return egcd(a, m)[1] % m

@njit(cache=True)
def crt2(r1, m1, r2, m2):
    inv = mod_inv(m1, m2)
    diff = (r2 - r1) % m2
    return r1 + m1 * ((diff * inv) % m2)

@njit(cache=True)
def crt3(r1, m1, r2, m2, r3, m3):
    x12 = crt2(r1, m1, r2, m2)
    return crt2(x12, m1 * m2, r3, m3)

def workload():
    # Same as crt.cell run()
    crt3(2, 3, 3, 5, 2, 7)
    crt3(1, 4, 2, 9, 3, 25)
    crt2(0, 10, 7, 13)

def warmup():
    try: egcd(2, 2)
    except Exception: pass
    try: mod_inv(2, 2)
    except Exception: pass
    try: crt2(2, 2, 2, 2)
    except Exception: pass
    try: crt3(2, 2, 2, 2, 2, 2)
    except Exception: pass

inner(workload, warmup=warmup)