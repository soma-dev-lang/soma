from _inner import inner
def trib(n):
    if n == 0: return 0
    if n == 1: return 0
    if n == 2: return 1
    a, b, c = 0, 0, 1
    for _ in range(3, n + 1):
        a, b, c = b, c, a + b + c
    return c

def workload():
    # Same as tribonacci.cell run()
    for n in (0, 3, 10, 20, 50, 100, 200):
        trib(n)

inner(workload)
