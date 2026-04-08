from _inner import inner
from numba import njit

@njit(cache=True)
def arctan(num, den, terms):
    x = num / den
    x2 = x * x
    term = x
    result = 0.0
    sign = 1.0
    for k in range(terms):
        result += sign * term / (2 * k + 1)
        term *= x2
        sign = -sign
    return result

@njit(cache=True)
def machin_pi():
    return 16.0 * arctan(1, 5, 30) - 4.0 * arctan(1, 239, 10)

def workload():
    machin_pi()

def warmup():
    try: arctan(2, 2, 2)
    except Exception: pass
    try: machin_pi()
    except Exception: pass

inner(workload, warmup=warmup)