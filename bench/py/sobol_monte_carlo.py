from _inner import inner
from math import sqrt

def estimate_pi(n):
    inside = 0
    state = 12345
    for _ in range(n):
        state ^= (state << 13) & 0xFFFFFFFFFFFFFFFF
        state ^= (state >> 7)
        state ^= (state << 17) & 0xFFFFFFFFFFFFFFFF
        x = (state % 1000000) / 1000000.0
        state ^= (state << 13) & 0xFFFFFFFFFFFFFFFF
        state ^= (state >> 7)
        state ^= (state << 17) & 0xFFFFFFFFFFFFFFFF
        y = (state % 1000000) / 1000000.0
        if x*x + y*y <= 1.0:
            inside += 1
    return 4.0 * inside / n

def workload():
    estimate_pi(1_000_000)

inner(workload)
