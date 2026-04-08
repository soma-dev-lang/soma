from _inner import inner

def rand(state):
    state ^= (state << 13) & 0xFFFFFFFFFFFFFFFF
    state ^= (state >> 7)
    state ^= (state << 17) & 0xFFFFFFFFFFFFFFFF
    return state

def simulate(steps, trials, seed):
    state = seed
    total = 0.0
    for _ in range(trials):
        pos = 0
        for _ in range(steps):
            state = rand(state)
            pos += 1 if state % 2 == 0 else -1
        total += abs(pos)
    return total / trials

def workload():
    simulate(100, 10000, 42)

inner(workload)
