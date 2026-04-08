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

def length_at(n):
    cur = "1"
    for _ in range(n):
        cur = next_term(cur)
    return len(cur)

def workload():
    length_at(4)
    length_at(10)
    length_at(20)

inner(workload)
