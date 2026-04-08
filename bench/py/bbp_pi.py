from _inner import inner

def bbp_pi(n_terms):
    """BBP series as a Float — gives ≈ 16 digits of accuracy."""
    s = 0.0
    for k in range(n_terms):
        s += (1.0 / 16**k) * (
            4.0 / (8 * k + 1)
            - 2.0 / (8 * k + 4)
            - 1.0 / (8 * k + 5)
            - 1.0 / (8 * k + 6)
        )
    return s

def workload():
    bbp_pi(100)

inner(workload)
