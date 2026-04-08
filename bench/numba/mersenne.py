# Numba's int is fixed-width int64. Lucas-Lehmer for p > 62 needs
# arbitrary precision (the values reach ~2^p bits). Python int is the
# only honest path here — Numba's @njit would compute garbage.
from _inner import inner


def lucas_lehmer(p):
    if p == 2: return True
    m = (1 << p) - 1
    s = 4
    for _ in range(p - 2):
        s = (s * s - 2) % m
    return s == 0


def workload():
    # Match the cell's full run() workload exactly.
    for (p, expected) in [
        (521, 1), (607, 1), (1279, 1), (2203, 1), (2281, 1),
        (3217, 1), (4253, 1), (4423, 1),
        (67, 0), (257, 0), (1009, 0), (1013, 0), (2999, 0), (4001, 0),
    ]:
        r = lucas_lehmer(p)
        assert int(r) == expected, f"p={p}: got {r}, expected {expected}"
    # Headline runs
    lucas_lehmer(9689)
    lucas_lehmer(11213)


inner(workload)
