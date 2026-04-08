"""Tiny helper for the bench: time a callable and print INNER:<ms> on stderr."""
import sys, time

def inner(fn, *args, **kwargs):
    t0 = time.perf_counter()
    fn(*args, **kwargs)
    elapsed = (time.perf_counter() - t0) * 1000
    print(f"INNER:{elapsed:.0f}", file=sys.stderr)
