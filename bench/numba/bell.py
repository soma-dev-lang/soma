# Numba's int is fixed-width int64. This cell needs arbitrary precision
# (values exceed 2^63), so we use plain Python int — Numba would
# silently overflow and return garbage in microseconds.
from _inner import inner
def bell(n):
    if n == 0: return 1
    row = [1]
    for _ in range(n):
        new_row = [row[-1]]
        for x in row:
            new_row.append(new_row[-1] + x)
        row = new_row
    return row[0]

def workload():
    # Same as bell.cell run()
    for n in (0, 1, 2, 5, 10, 15, 20, 25, 30):
        bell(n)

inner(workload)
