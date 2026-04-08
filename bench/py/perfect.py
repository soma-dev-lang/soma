from _inner import inner
def lucas_lehmer(p):
    if p == 2: return 1
    m = (1 << p) - 1
    s = 4
    for _ in range(p - 2):
        s = (s * s - 2) % m
    return 1 if s == 0 else 0

def perfect_from_p(p):
    return (1 << (p - 1)) * ((1 << p) - 1)

def workload():
    # Same workload as perfect.cell run()
    for p in (2, 3, 5, 7, 13, 17, 19):
        perfect_from_p(p)
    found = sum(lucas_lehmer(p) for p in range(2, 32))

inner(workload)
