from _inner import inner
from math import sin

def rand_float(state):
    state ^= (state << 13) & 0xFFFFFFFFFFFFFFFF
    state ^= (state >> 7)
    state ^= (state << 17) & 0xFFFFFFFFFFFFFFFF
    return state

def simulate(trials, seed):
    state = seed
    cross = 0
    for _ in range(trials):
        state = rand_float(state)
        d = ((state % 1000000) / 1000000.0) * 0.5
        state = rand_float(state)
        theta = ((state % 1000000) / 1000000.0) * 1.5707963267948966
        if 0.5 * sin(theta) > d:
            cross += 1
    return (2.0 * trials) / cross if cross else 0.0

def workload():
    simulate(1_000_000, 12345)

inner(workload)
