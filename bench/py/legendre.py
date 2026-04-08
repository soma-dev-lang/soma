from _inner import inner
def legendre(a, p):
    r = a % p
    if r == 0: return 0
    e = (p - 1) // 2
    v = pow(r, e, p)
    return 1 if v == 1 else -1

def workload():
    # Same as legendre.cell run()
    for (a, p) in [(1,7), (2,7), (3,7), (4,7), (5,7), (6,7), (3,5), (5,3), (7,11), (11,7)]:
        legendre(a, p)

inner(workload)
