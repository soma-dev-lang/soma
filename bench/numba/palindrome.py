from _inner import inner
def is_palindrome(s):
    return s == s[::-1]

def is_num_palindrome(n):
    return is_palindrome(str(n))

def largest_palindrome_3digit():
    best = 0
    for a in range(999, 99, -1):
        for b in range(999, a - 1, -1):
            p = a * b
            if p > best and is_num_palindrome(p):
                best = p
    return best

def workload():
    # Same as palindrome.cell run()
    for s in ("racecar", "noon", "a", "hello"):
        is_palindrome(s)
    is_num_palindrome(12321)
    is_num_palindrome(12345)
    largest_palindrome_3digit()

inner(workload)
