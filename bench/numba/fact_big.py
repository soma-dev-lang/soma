from _inner import inner

from math import factorial

def workload():
    for n in (50, 100, 200, 500):
        factorial(n)

inner(workload)
