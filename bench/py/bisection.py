from _inner import inner
def f(x): return x**3 - x - 2

def bisect(a, b, tol):
    lo, hi = a, b
    for _ in range(200):
        mid = (lo + hi) / 2
        fm = f(mid)
        if abs(fm) < tol or abs(hi - lo) < tol: return mid
        if (f(lo) < 0) == (fm < 0):
            lo = mid
        else:
            hi = mid
    return (lo + hi) / 2

def workload():
    bisect(1.0, 2.0, 1e-9)

inner(workload)
