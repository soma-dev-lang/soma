from _inner import inner
def is_prime(n):
    if n < 2: return 0
    if n < 4: return 1
    if n % 2 == 0: return 0
    i = 3
    while i * i <= n:
        if n % i == 0: return 0
        i += 2
    return 1

def primorial(p):
    r = 1
    for i in range(2, p + 1):
        if is_prime(i):
            r *= i
    return r

def workload():
    # Same as primorial.cell run()
    for p in (2, 5, 11, 17, 29, 41, 53, 97):
        primorial(p)

inner(workload)
