from _inner import inner

def gcd_iter(a, b):
    while b:
        a, b = b, a % b
    return a

def workload():
    for i in range(1, 200):
        a = i * 7919
        b = (i + 3) * 6577
        gcd_iter(a, b)
    gcd_iter(832040, 1346269)

inner(workload)
