from _inner import inner
def partition(n):
    if n < 0: return 0
    p = [0] * (n + 1)
    p[0] = 1
    for i in range(1, n + 1):
        total = 0
        k = 1
        while True:
            g1 = k * (3 * k - 1) // 2
            if g1 > i: break
            term = p[i - g1]
            g2 = k * (3 * k + 1) // 2
            if g2 <= i:
                term += p[i - g2]
            if k % 2 == 1:
                total += term
            else:
                total -= term
            k += 1
        p[i] = total
    return p[n]

def workload():
    # Same as partitions.cell run()
    for n in (0, 1, 5, 10, 20, 50, 100):
        partition(n)

inner(workload)
