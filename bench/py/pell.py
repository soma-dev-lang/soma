from _inner import inner
from math import isqrt

def pell(nn):
    a0 = isqrt(nn)
    if a0 * a0 == nn: return "0 0"
    m, d, a = 0, 1, a0
    h_prev, h_cur = 1, a0
    k_prev, k_cur = 0, 1
    while True:
        m = d * a - m
        d = (nn - m * m) // d
        a = (a0 + m) // d
        h_next = a * h_cur + h_prev
        k_next = a * k_cur + k_prev
        h_prev, h_cur = h_cur, h_next
        k_prev, k_cur = k_cur, k_next
        if h_cur * h_cur - nn * k_cur * k_cur == 1:
            break
    return f"{h_cur} {k_cur}"

def workload():
    for d in (2, 3, 5, 6, 7, 13, 29, 41, 61, 109):
        pell(d)

inner(workload)
