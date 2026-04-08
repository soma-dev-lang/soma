from _inner import inner

def collatz_length(n):
    length = 1
    v = n
    while v != 1:
        if v % 2 == 0:
            v //= 2
        else:
            v = 3 * v + 1
        length += 1
    return length

def longest_under(limit):
    best_start, best_len = 1, 1
    for i in range(2, limit):
        l = collatz_length(i)
        if l > best_len:
            best_len, best_start = l, i
    return best_start

def workload():
    for limit in (100, 1000, 10000, 100000, 1000000):
        longest_under(limit)

inner(workload)
