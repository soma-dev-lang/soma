from _inner import inner

def tonelli_shanks(n, p):
    """Find r such that r² ≡ n (mod p) for odd prime p."""
    if pow(n, (p - 1) // 2, p) != 1: return None
    if p % 4 == 3: return pow(n, (p + 1) // 4, p)
    q, s = p - 1, 0
    while q % 2 == 0:
        q //= 2
        s += 1
    z = 2
    while pow(z, (p - 1) // 2, p) != p - 1: z += 1
    m, c, t, r = s, pow(z, q, p), pow(n, q, p), pow(n, (q + 1) // 2, p)
    while t != 1:
        i, t2 = 0, t
        while t2 != 1:
            t2 = (t2 * t2) % p
            i += 1
        b = pow(c, 1 << (m - i - 1), p)
        m, c, t, r = i, (b * b) % p, (t * b * b) % p, (r * b) % p
    return r

def workload():
    for (n, p) in [(2, 7), (3, 11), (10, 13), (5, 17), (2, 1000003), (123456, 999983)]:
        tonelli_shanks(n, p)

inner(workload)
