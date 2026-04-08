from _inner import inner
def binom(n, k):
    if k < 0 or k > n: return 0
    if k > n - k: k = n - k
    r = 1
    for i in range(1, k + 1):
        r = r * (n - i + 1) // i
    return r

def workload():
    # Same as binomial.cell run()
    for (n, k) in [(0,0), (5,2), (10,5), (20,10), (50,25), (100,50), (200,100), (500,250)]:
        binom(n, k)

inner(workload)
