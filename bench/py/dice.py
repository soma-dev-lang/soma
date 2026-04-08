from _inner import inner

def ways(n, f, s):
    dp = [0] * (n * f + 1)
    dp[0] = 1
    for _ in range(n):
        new_dp = [0] * (n * f + 1)
        for t in range(len(new_dp)):
            for r in range(1, f + 1):
                if t - r >= 0:
                    new_dp[t] += dp[t - r]
        dp = new_dp
    if s < n or s > n * f: return 0
    return dp[s]

def workload():
    ways(2, 6, 7)
    ways(3, 6, 10)
    ways(4, 6, 14)
    sum(ways(3, 6, s) for s in range(3, 19))

inner(workload)
