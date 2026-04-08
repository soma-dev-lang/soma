from _inner import inner
from numba import njit

@njit(cache=True)
def build_failure(pat):
    m = len(pat)
    fail = [0] * m
    k = 0
    for i in range(1, m):
        while k > 0 and pat[k] != pat[i]:
            k = fail[k-1]
        if pat[k] == pat[i]:
            k += 1
        fail[i] = k
    return fail

@njit(cache=True)
def kmp_find(text, pat):
    if not pat: return 0
    fail = build_failure(pat)
    q = 0
    for i, c in enumerate(text):
        while q > 0 and pat[q] != c:
            q = fail[q-1]
        if pat[q] == c:
            q += 1
        if q == len(pat):
            return i - len(pat) + 1
    return -1

def workload():
    for (t, p) in [("hello world","world"), ("abcdefgh","def"), ("aaaaaab","aab"),
                   ("ababcabcabababd","ababd"), ("missing","xyz"), ("abc","")]:
        kmp_find(t, p)

def warmup():
    try: build_failure(2)
    except Exception: pass
    try: kmp_find(2, 2)
    except Exception: pass

inner(workload, warmup=warmup)