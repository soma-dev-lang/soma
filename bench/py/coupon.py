from _inner import inner

def expected_coupons(n):
    return n * sum(1.0 / k for k in range(1, n + 1))

def rand(state):
    state ^= (state << 13) & 0xFFFFFFFFFFFFFFFF
    state ^= (state >> 7)
    state ^= (state << 17) & 0xFFFFFFFFFFFFFFFF
    return state

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

inner(workload)
