from _inner import inner
def padovan(n):
    if n < 3: return 1
    a, b, c = 1, 1, 1
    for _ in range(3, n + 1):
        a, b, c = b, c, a + b
    return c

def workload():
    # Same as padovan.cell run()
    for n in (0, 5, 10, 20, 50, 100, 500):
        padovan(n)

inner(workload)
