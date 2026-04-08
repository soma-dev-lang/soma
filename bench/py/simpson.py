from _inner import inner
from math import sin, pi

def simpson(a, b, n):
    h = (b - a) / n
    s = sin(a) + sin(b)
    for i in range(1, n):
        x = a + i * h
        s += (4 if i % 2 else 2) * sin(x)
    return s * h / 3

def workload():
    simpson(0.0, pi, 10)
    simpson(0.0, pi, 100)
    simpson(0.0, pi, 1000)

inner(workload)
