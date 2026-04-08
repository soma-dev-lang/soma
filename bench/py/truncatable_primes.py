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

def is_truncatable(n):
    if n < 10: return False
    s = str(n)
    for i in range(len(s)):
        if not is_prime(int(s[i:])): return False
    for i in range(len(s)):
        if not is_prime(int(s[:len(s) - i])): return False
    return True

def sum_truncatables():
    total, count, n = 0, 0, 11
    while count < 11:
        if is_truncatable(n):
            total += n
            count += 1
        n += 2
    return total

def workload():
    sum_truncatables()

inner(workload)
