from _inner import inner

def leibniz_pi(n):
    s = 0.0
    sign = 1.0
    for k in range(n):
        s += sign / (2 * k + 1)
        sign = -sign
    return 4.0 * s

def workload():
    leibniz_pi(10_000_000)

inner(workload)
