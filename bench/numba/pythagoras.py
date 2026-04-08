from _inner import inner
from numba import njit


@njit(cache=True)
def pe9():
    for a in range(1, 1000):
        for b in range(a + 1, 1000 - a):
            c = 1000 - a - b
            if c < b: break
            if a*a + b*b == c*c: return a*b*c
    return 0

@njit(cache=True)
def count_triples(perimeter):
    count = 0
    for a in range(1, perimeter // 3):
        for b in range(a + 1, perimeter // 2):
            c = perimeter - a - b
            if c <= b: break
            if a*a + b*b == c*c: count += 1
    return count

def workload():
    pe9()
    count_triples(120)

def warmup():
    try: pe9()
    except Exception: pass
    try: count_triples(2)
    except Exception: pass

inner(workload, warmup=warmup)