from _inner import inner

from math import factorial

def workload():
    sum(int(c) for c in str(factorial(100)))
    sum(int(c) for c in str(factorial(1000)))

inner(workload)
