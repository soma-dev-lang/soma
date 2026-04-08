from _inner import inner

def wallis_pi(n):
    p = 1.0
    for k in range(1, n + 1):
        kk = k * k
        p *= (4.0 * kk) / (4.0 * kk - 1.0)
    return 2.0 * p

def workload():
    wallis_pi(1_000_000)

inner(workload)
