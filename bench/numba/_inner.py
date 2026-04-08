"""Numba bench harness — same INNER:N output protocol as bench/py/_inner.py.

Supports both calling conventions:
  inner(fn)                       # legacy CPython style
  inner(fn, warmup=warmup_fn)     # Numba style: warmup excluded from inner

Numba JIT compile happens on first call. To get a fair "computation only"
inner timing, the @njit cells call `inner(workload, warmup=warmup_fn)`
where warmup_fn triggers JIT compilation BEFORE the timed window opens.
The wall-clock measurement (via bench/time_ms.py) still pays the JIT
cost on a cold cache, so we also rely on Numba's `@njit(cache=True)`
to persist compiled artifacts to __pycache__/.numba_cache.
"""
import sys
import time


def inner(fn, *args, warmup=None, **kwargs):
    if warmup is not None:
        warmup()
    t0 = time.perf_counter()
    fn(*args, **kwargs)
    elapsed = (time.perf_counter() - t0) * 1000
    # Print integer ms (compat) AND microsecond-precision INNER_US for
    # cells too fast to resolve at ms granularity.
    print(f"INNER:{int(round(elapsed))}", file=sys.stderr)
    print(f"INNER_US:{int(round(elapsed * 1000))}", file=sys.stderr)
