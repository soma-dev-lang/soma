from _inner import inner
def egcd(a, b):
    if b == 0: return (a, 1, 0)
    g, x1, y1 = egcd(b, a % b)
    return (g, y1, x1 - (a // b) * y1)

def mod_inverse(a, m):
    g, x, _ = egcd(a, m)
    return x % m

def workload():
    # Same as ext_gcd.cell run()
    for (a, b) in [(240, 46), (101, 13), (1071, 462)]:
        egcd(a, b)
    for (a, m) in [(3, 11), (7, 26), (17, 3120), (65537, 1000003)]:
        mod_inverse(a, m)

inner(workload)
