from _inner import inner
def arctan(num, den, terms):
    x = num / den
    x2 = x * x
    term = x
    result = 0.0
    sign = 1.0
    for k in range(terms):
        result += sign * term / (2 * k + 1)
        term *= x2
        sign = -sign
    return result

def machin_pi():
    return 16.0 * arctan(1, 5, 30) - 4.0 * arctan(1, 239, 10)

def workload():
    machin_pi()

inner(workload)
