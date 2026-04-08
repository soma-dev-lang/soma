from _inner import inner

def hq(n, cache={}):
    if n in cache: return cache[n]
    if n <= 2: return 1
    r = hq(n - hq(n - 1)) + hq(n - hq(n - 2))
    cache[n] = r
    return r

def workload():
    for n in (1, 5, 10, 50, 100, 500):
        hq(n)

inner(workload)
