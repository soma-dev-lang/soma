from _inner import inner
def phi(n):
    if n <= 1: return n
    result = n
    m = n
    p = 2
    while p * p <= m:
        if m % p == 0:
            while m % p == 0:
                m //= p
            result -= result // p
        p += 1
    if m > 1:
        result -= result // m
    return result

def workload():
    # Same workload as totient.cell run()
    for n in (1, 2, 9, 10, 36, 100, 997, 1000000):
        phi(n)
    sum(phi(k) for k in range(1, 10001))

inner(workload)
