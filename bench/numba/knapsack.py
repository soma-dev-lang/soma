from _inner import inner
from numba import njit

@njit(cache=True)
def knapsack(capacity, items):
    dp = [0] * (capacity + 1)
    for w, v in items:
        for c in range(capacity, w - 1, -1):
            if dp[c - w] + v > dp[c]:
                dp[c] = dp[c - w] + v
    return dp[capacity]

def workload():
    items = [(2,3),(3,4),(4,5),(5,8),(9,10)]
    knapsack(20, items)
    knapsack(10, items)

def warmup():
    try: knapsack(2, 2)
    except Exception: pass

inner(workload, warmup=warmup)