from _inner import inner
def heapsort(arr):
    import heapq
    heapq.heapify(arr)
    return [heapq.heappop(arr) for _ in range(len(arr))]

def lcg(n, seed):
    s = seed
    out = []
    for _ in range(n):
        s = (s * 1103515245 + 12345) % 2147483648
        out.append(s % 100)
    return out

def workload():
    heapsort(lcg(25, 42))
    heapsort(lcg(32, 12345))

inner(workload)
