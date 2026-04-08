from _inner import inner

def fib_matrix(n):
    if n == 0: return 0
    if n == 1: return 1
    ra, rb, rc, rd = 1, 0, 0, 1
    ba, bb, bc, bd = 1, 1, 1, 0
    m = n
    while m > 0:
        if m % 2 == 1:
            na = ra*ba + rb*bc
            nb = ra*bb + rb*bd
            nc = rc*ba + rd*bc
            nd = rc*bb + rd*bd
            ra, rb, rc, rd = na, nb, nc, nd
        sa = ba*ba + bb*bc
        sb = ba*bb + bb*bd
        sc = bc*ba + bd*bc
        sd = bc*bb + bd*bd
        ba, bb, bc, bd = sa, sb, sc, sd
        m //= 2
    return rb

def workload():
    for n in (0, 1, 2, 10, 20, 50, 100, 1000, 10000, 100000, 1000000):
        fib_matrix(n)

inner(workload)
