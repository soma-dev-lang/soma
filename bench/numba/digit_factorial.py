from _inner import inner
from math import factorial

FACTS = [factorial(i) for i in range(10)]

def sdf(n):
    s = 0
    while n > 0:
        s += FACTS[n % 10]
        n //= 10
    return s

def pe34():
    total = 0
    for n in range(3, 2540160):
        if sdf(n) == n:
            total += n
    return total

def workload():
    pe34()

inner(workload)
