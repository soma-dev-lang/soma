from _inner import inner
def ways(target, coins=(1,2,5,10,20,50,100,200)):
    dp = [0] * (target + 1)
    dp[0] = 1
    for c in coins:
        for a in range(c, target + 1):
            dp[a] += dp[a - c]
    return dp[target]

def workload():
    ways(10); ways(50); ways(200)

inner(workload)
