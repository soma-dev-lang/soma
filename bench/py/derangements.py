from _inner import inner
def derange(n):
    if n == 0: return 1
    if n == 1: return 0
    a, b = 1, 0
    for i in range(2, n + 1):
        a, b = b, (i - 1) * (a + b)
    return b

def workload():
    # Same as derangements.cell run()
    for n in (0, 1, 2, 3, 4, 5, 10, 20, 30, 50):
        derange(n)

inner(workload)
