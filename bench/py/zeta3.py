from _inner import inner

def zeta3(n):
    return sum(1.0 / (k * k * k) for k in range(1, n + 1))

def workload():
    zeta3(1_000_000)

inner(workload)
