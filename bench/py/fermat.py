from _inner import inner

def fermat_test(n):
    if n < 2: return 0
    if n in (2, 3): return 1
    if n % 2 == 0: return 0
    if pow(2, n-1, n) != 1: return 0
    if pow(3, n-1, n) != 1: return 0
    if pow(5, n-1, n) != 1: return 0
    if pow(7, n-1, n) != 1: return 0
    return 1

def workload():
    sum(fermat_test(n) for n in range(2, 10001))

inner(workload)
