from _inner import inner


def champernowne_str(n):
    out = []
    i = 1
    while sum(len(s) for s in out) < n:
        out.append(str(i))
        i += 1
    return ''.join(out)[:n]

def workload():
    for n in (10, 100, 1000, 10000):
        champernowne_str(n)

inner(workload)
