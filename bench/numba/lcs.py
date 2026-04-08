from _inner import inner
from numba import njit

@njit(cache=True)
def lcs_len(a, b):
    m, n = len(a), len(b)
    prev = [0] * (n + 1)
    for i in range(1, m + 1):
        cur = [0] * (n + 1)
        for j in range(1, n + 1):
            if a[i-1] == b[j-1]:
                cur[j] = prev[j-1] + 1
            else:
                cur[j] = max(prev[j], cur[j-1])
        prev = cur
    return prev[n]

def workload():
    for (a, b) in [("ABCBDAB","BDCABA"), ("AGGTAB","GXTXAYB"), ("abcdef","acf"),
                   ("hello","world"), ("","abc"), ("xyzzy","xyzzy")]:
        lcs_len(a, b)

def warmup():
    try: lcs_len(2, 2)
    except Exception: pass

inner(workload, warmup=warmup)