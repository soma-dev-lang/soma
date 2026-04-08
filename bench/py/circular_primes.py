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

def is_circular(n):
    s = str(n)
    for i in range(len(s)):
        if not is_prime(int(s[i:] + s[:i])):
            return False
    return True

def count_circular(limit):
    return sum(1 for n in range(2, limit) if is_circular(n))

def workload():
    count_circular(100)
    count_circular(1_000_000)

inner(workload)
