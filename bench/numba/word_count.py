from _inner import inner
from numba import njit


@njit(cache=True)
def word_count(s):
    return len(s.split())

def workload():
    word_count("hello world this is a test of the word counter")
    word_count("a b c d e f g h i j k l m n o p q r s t")
    word_count("singleword")

def warmup():
    try: word_count(2)
    except Exception: pass

inner(workload, warmup=warmup)