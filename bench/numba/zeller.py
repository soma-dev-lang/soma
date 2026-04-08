from _inner import inner
from numba import njit

@njit(cache=True)
def day_of_week(year, month, day):
    y, m = year, month
    if m < 3:
        m += 12
        y -= 1
    K = y % 100
    J = y // 100
    h = (day + (13 * (m + 1)) // 5 + K + K // 4 + J // 4 + 5 * J) % 7
    return ((h + 5) % 7) + 1

def workload():
    for (y, m, d) in [(2000,1,1),(2024,1,1),(2024,12,25),(1969,7,20),(1989,11,9),(2026,4,8)]:
        day_of_week(y, m, d)

def warmup():
    try: day_of_week(2, 2, 2)
    except Exception: pass

inner(workload, warmup=warmup)