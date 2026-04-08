from _inner import inner
from math import factorial as f

def multi3(k1, k2, k3):
    return f(k1+k2+k3) // (f(k1)*f(k2)*f(k3))

def multi4(k1, k2, k3, k4):
    return f(k1+k2+k3+k4) // (f(k1)*f(k2)*f(k3)*f(k4))

def workload():
    # Same as multinomial.cell run()
    multi3(2, 2, 0)
    multi3(2, 3, 5); multi3(5, 5, 5); multi3(10, 10, 10)
    multi4(10, 10, 10, 10)

inner(workload)
