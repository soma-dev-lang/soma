from _inner import inner


def next_term(s):
    out = []
    i = 0
    n = len(s)
    while i < n:
        c = s[i]
        r = 1
        while i + r < n and s[i + r] == c:
            r += 1
        out.append(str(r))
        out.append(c)
        i += r
    return ''.join(out)

def workload():
    cur = "1"
    for _ in range(50):
        cur = next_term(cur)

inner(workload)
