from _inner import inner
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

inner(workload)
