from _inner import inner

def e_digits(n):
    """Spigot algorithm for digits of e."""
    a = [1] * (n + 2)
    out = ['2', '.']
    for _ in range(n):
        # Multiply by 10
        carry = 0
        for j in range(n + 1, 0, -1):
            x = a[j] * 10 + carry
            a[j] = x % (j + 1)
            carry = x // (j + 1)
        out.append(str(carry))
    return ''.join(out)

def workload():
    e_digits(1000)

inner(workload)
