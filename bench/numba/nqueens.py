from _inner import inner
from numba import njit


@njit(cache=True)
def solve(n, row, col_mask, d1, d2):
    if row == n: return 1
    count = 0
    blocked = col_mask | d1 | d2
    full = (1 << n) - 1
    free = full & ~blocked
    while free:
        bit = free & -free
        free -= bit
        count += solve(n, row + 1, col_mask | bit, (d1 | bit) << 1, (d2 | bit) >> 1)
    return count

@njit(cache=True)
def count_queens(n):
    return solve(n, 0, 0, 0, 0)

def workload():
    # Match the cell: N=1..15. nqueens is recursion-bound; both languages
    # spend nearly all their time inside solve() and the per-call overhead
    # difference between Soma's compiled Rust and CPython's bytecode
    # interpreter is what the comparison measures.
    for n in (1, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15):
        count_queens(n)

def warmup():
    try: solve(2, 2, 2, 2, 2)
    except Exception: pass
    try: count_queens(2)
    except Exception: pass

inner(workload, warmup=warmup)