from _inner import inner
def mergesort(arr):
    if len(arr) <= 1: return arr
    mid = len(arr) // 2
    left = mergesort(arr[:mid])
    right = mergesort(arr[mid:])
    out = []
    i = j = 0
    while i < len(left) and j < len(right):
        if left[i] <= right[j]:
            out.append(left[i]); i += 1
        else:
            out.append(right[j]); j += 1
    out.extend(left[i:])
    out.extend(right[j:])
    return out

def workload():
    mergesort(list(range(20, 0, -1)))
    mergesort(list(range(32, 0, -1)))

inner(workload)
