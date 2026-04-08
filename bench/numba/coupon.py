from _inner import inner
from numba import njit


def expected_coupons(n):
    return n * sum(1.0 / k for k in range(1, n + 1))

@njit(cache=True)
def rand(state):
    state ^= (state << 13) & 0xFFFFFFFFFFFFFFFF
    state ^= (state >> 7)
    state ^= (state << 17) & 0xFFFFFFFFFFFFFFFF
    return state

@njit(cache=True)
def simulate(n, trials, seed):
    state = seed
    total = 0.0
    for _ in range(trials):
        seen = 0
        count = 0
        full = (1 << n) - 1
        while seen != full:
            state = rand(state)
            bin_ = state % n
            seen |= (1 << bin_)
            count += 1
        total += count
    return total / trials

def workload():
    expected_coupons(10)
    simulate(10, 5000, 12345)

def warmup():
    try: rand(2)
    except Exception: pass
    try: simulate(2, 2, 2)
    except Exception: pass

inner(workload, warmup=warmup)