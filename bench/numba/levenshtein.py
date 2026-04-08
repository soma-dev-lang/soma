from _inner import inner
from numba import njit


# Same recursive algorithm as the cell — no memoization. Exponential
# but matches the cell's workload exactly so the comparison is fair.

@njit(cache=True)
def lev(a, b, i, j):
    if i == 0: return j
    if j == 0: return i
    if a[i - 1] == b[j - 1]:
        return lev(a, b, i - 1, j - 1)
    d1 = lev(a, b, i - 1, j)
    d2 = lev(a, b, i, j - 1)
    d3 = lev(a, b, i - 1, j - 1)
    return 1 + min(d1, d2, d3)

@njit(cache=True)
def edit_distance(a, b):
    return lev(a, b, len(a), len(b))

def workload():
    edit_distance("", "")
    edit_distance("a", "")
    edit_distance("", "abc")
    edit_distance("kitten", "sitting")
    edit_distance("flaw", "lawn")
    edit_distance("intention", "execution")
    edit_distance("Saturday", "Sunday")
    edit_distance("abcdefghi", "ihgfedcba")

def warmup():
    try: lev(2, 2, 2, 2)
    except Exception: pass
    try: edit_distance(2, 2)
    except Exception: pass

inner(workload, warmup=warmup)