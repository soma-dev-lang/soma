from _inner import inner

def spiral_sum(side):
    total = 1
    max_k = (side - 1) // 2
    for k in range(1, max_k + 1):
        n = 2 * k + 1
        total += 4 * n * n - 12 * k
    return total

def workload():
    spiral_sum(5)
    spiral_sum(1001)

inner(workload)
