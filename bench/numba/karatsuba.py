from _inner import inner

def karatsuba(x, y, bits):
    if bits <= 64:
        return x * y
    half = bits // 2
    x_l = x & ((1 << half) - 1)
    x_h = x >> half
    y_l = y & ((1 << half) - 1)
    y_h = y >> half
    z0 = karatsuba(x_l, y_l, half + 1)
    z2 = karatsuba(x_h, y_h, half + 1)
    z1 = karatsuba(x_h + x_l, y_h + y_l, half + 2) - z0 - z2
    return (z2 << (2 * half)) + (z1 << half) + z0

def workload():
    karatsuba(13, 17, 64)
    karatsuba(123456789, 987654321, 64)
    a = (1 << 100) + 12345
    b = (1 << 100) - 67890
    karatsuba(a, b, 102)
    c = (1 << 500) + 1
    d = (1 << 500) - 1
    karatsuba(c, d, 502)
    e = (1 << 1000) + (1 << 500)
    f = (1 << 1000) - (1 << 500)
    karatsuba(e, f, 1002)

inner(workload)
