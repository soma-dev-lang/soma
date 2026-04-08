"""Time a subprocess to millisecond precision and print the elapsed ms.

Runs the command 3 times and reports the BEST (minimum) elapsed time.
This filters startup-jitter and disk-cache noise — for tiny workloads
where Soma vs Python differs by a few ms, taking the min gives a
more reproducible signal than a single run.

Hard 60-second timeout per run to prevent any single bench from
hanging the comparison harness."""
import subprocess, sys, time

cmd = sys.argv[1:]
N = 5

best = None
for _ in range(N):
    t0 = time.perf_counter()
    try:
        subprocess.run(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=60)
    except subprocess.TimeoutExpired:
        print("TIMEOUT")
        sys.exit(0)
    elapsed_ms = (time.perf_counter() - t0) * 1000
    if best is None or elapsed_ms < best:
        best = elapsed_ms

print(f"{best:.0f}")
