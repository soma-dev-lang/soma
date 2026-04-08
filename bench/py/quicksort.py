from _inner import inner
def quicksort(arr, lo, hi):
    if lo < hi:
        pivot = arr[hi]
        i = lo - 1
        for j in range(lo, hi):
            if arr[j] <= pivot:
                i += 1
                arr[i], arr[j] = arr[j], arr[i]
        arr[i+1], arr[hi] = arr[hi], arr[i+1]
        p = i + 1
        quicksort(arr, lo, p - 1)
        quicksort(arr, p + 1, hi)

def workload():
    for n in (20, 32):
        arr = list(range(n, 0, -1))
        quicksort(arr, 0, n - 1)

inner(workload)
