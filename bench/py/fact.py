from _inner import inner
from math import factorial

def workload():
    for n in (5, 10, 15, 20):
        factorial(n)

inner(workload)
