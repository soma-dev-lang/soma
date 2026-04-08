from _inner import inner
def histogram(s):
    h = [0] * 26
    for c in s.lower():
        if 'a' <= c <= 'z':
            h[ord(c) - 97] += 1
    return tuple(h)

def is_anagram(a, b):
    return histogram(a) == histogram(b)

def workload():
    for (a, b) in [("listen","silent"), ("triangle","integral"), ("conversation","voicesranton"),
                   ("apple","papel"), ("hello","world"), ("abc","abcd")]:
        is_anagram(a, b)

inner(workload)
