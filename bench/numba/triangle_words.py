from _inner import inner
from math import isqrt

def letter_sum(s):
    return sum((ord(c.upper()) - 64) for c in s if c.isalpha())

def is_triangular(x):
    s = 1 + 8 * x
    r = isqrt(s)
    return r * r == s and (r - 1) % 2 == 0

def workload():
    for name in ("SKY","CAT","TREE","AB","ABC","ABCD","BIG","HAT"):
        is_triangular(letter_sum(name))

inner(workload)
