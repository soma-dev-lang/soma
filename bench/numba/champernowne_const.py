from _inner import inner


def digit_at(n):
    rem, k, block_size, first = n, 1, 9, 1
    while rem > block_size * k:
        rem -= block_size * k
        k += 1
        block_size *= 10
        first *= 10
    idx = (rem - 1) // k
    pos = (rem - 1) % k
    num = first + idx
    return int(str(num)[pos])

def workload():
    for i in (1, 10, 100, 1000, 10000, 100000, 1000000):
        digit_at(i)

inner(workload)
