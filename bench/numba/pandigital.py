from _inner import inner


def has_unique_digits(s):
    return '0' not in s and len(set(s)) == len(s)

def pe32():
    seen = set()
    total = 0
    for a in range(1, 100):
        for b in range(a + 1, 10000):
            c = a * b
            if c > 9999: break
            s = f"{a}{b}{c}"
            if len(s) == 9 and set(s) == set("123456789"):
                if c not in seen:
                    seen.add(c)
                    total += c
    return total

def workload():
    pe32()

inner(workload)
