from _inner import inner
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

inner(workload)
