from _inner import inner

def workload():
    n = 100
    sum_sq = sum(i * i for i in range(1, n + 1))
    sq_sum = (n * (n + 1) // 2) ** 2
    sq_sum - sum_sq

inner(workload)
