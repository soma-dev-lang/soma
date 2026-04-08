from _inner import inner
def recaman(n):
    seen = {0}
    cur = 0
    for i in range(1, n + 1):
        candidate = cur - i
        if candidate > 0 and candidate not in seen:
            cur = candidate
        else:
            cur = cur + i
        seen.add(cur)
    return cur

def workload():
    # Same as recaman.cell run()
    for n in (0, 1, 2, 4, 6, 10, 20, 50, 100, 500):
        recaman(n)

inner(workload)
