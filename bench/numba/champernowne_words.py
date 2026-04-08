from _inner import inner

ONES = {1:3,2:3,3:5,4:4,5:4,6:3,7:5,8:5,9:4,10:3,
        11:6,12:6,13:8,14:8,15:7,16:7,17:9,18:8,19:8}
TENS = {2:6,3:6,4:5,5:5,6:5,7:7,8:6,9:6}

def letters(n):
    if n == 1000: return 11
    if n >= 100:
        h, rem = divmod(n, 100)
        l = ONES[h] + 7
        if rem > 0: l += 3 + letters(rem)
        return l
    if n >= 20:
        t, r = divmod(n, 10)
        l = TENS[t]
        if r > 0: l += ONES[r]
        return l
    return ONES[n]

def workload():
    sum(letters(i) for i in range(1, 1001))

inner(workload)
