from _inner import inner
def search(arr, target):
    lo, hi = 0, len(arr) - 1
    while lo <= hi:
        mid = (lo + hi) // 2
        v = arr[mid]
        if v == target: return mid
        if v < target:
            lo = mid + 1
        else:
            hi = mid - 1
    return -1

def workload():
    arr = [i * 3 for i in range(100)]
    for t in (0, 297, 150, 7, 1000):
        search(arr, t)

inner(workload)
