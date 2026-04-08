from _inner import inner

def is_prime(n):
    if n < 2: return False
    if n < 4: return True
    if n % 2 == 0: return False
    i = 3
    while i * i <= n:
        if n % i == 0: return False
        i += 2
    return True

def consecutive(a, b):
    n = 0
    while True:
        v = n * n + a * n + b
        if v < 0 or not is_prime(v): return n
        n += 1

def pe27():
    best_count = 0
    best_prod = 0
    for a in range(-999, 1000):
        for b in range(-1000, 1001):
            if not is_prime(b): continue
            c = consecutive(a, b)
            if c > best_count:
                best_count = c
                best_prod = a * b
    return best_prod

def workload():
    consecutive(1, 41)
    consecutive(-79, 1601)
    pe27()

inner(workload)
