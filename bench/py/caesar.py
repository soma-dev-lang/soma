from _inner import inner
def encrypt(s, shift):
    out = []
    k = shift % 26
    if k < 0: k += 26
    for c in s:
        if 'a' <= c <= 'z':
            out.append(chr((ord(c) - 97 + k) % 26 + 97))
        else:
            out.append(' ')
    return ''.join(out)

def workload():
    encrypt("hello", 3)
    encrypt("xyz", 5)
    encrypt("khoor", -3)

inner(workload)
