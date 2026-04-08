from _inner import inner

def lucas_lehmer(p):
    if p == 2: return True
    m = (1 << p) - 1
    s = 4
    for _ in range(p - 2):
        s = (s * s - 2) % m
    return s == 0

def workload():
    for p in (257, 1009, 1013, 2999, 4001, 9689, 11213):
        lucas_lehmer(p)

inner(workload)
