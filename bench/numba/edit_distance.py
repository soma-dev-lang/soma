from _inner import inner

def edit(a, b):
    m, n = len(a), len(b)
    prev = list(range(n + 1))
    for i in range(1, m + 1):
        cur = [i] + [0] * n
        for k in range(1, n + 1):
            cost = 0 if a[i-1] == b[k-1] else 1
            cur[k] = min(prev[k] + 1, cur[k-1] + 1, prev[k-1] + cost)
        prev = cur
    return prev[n]

def workload():
    for (a, b) in [("kitten","sitting"), ("flaw","lawn"), ("intention","execution"),
                   ("abc","yabd"), ("","abc"), ("hello","hello")]:
        edit(a, b)

inner(workload)
