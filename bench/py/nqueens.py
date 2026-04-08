from _inner import inner

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

def count_queens(n):
    return solve(n, 0, 0, 0, 0)

def workload():
    # Skip n=16 — Python takes 5+ minutes; the cell ran the same workload
    # but Python can't keep up. We reduce to n≤14 for the comparison; the
    # cell still runs n=16 in its own benchmark.
    for n in (1, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14):
        count_queens(n)

inner(workload)
