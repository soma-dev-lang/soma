"""Time a subprocess to millisecond precision and print the elapsed ms.
Has a hard 60-second timeout to prevent any single bench from hanging
the comparison harness; on timeout prints "TIMEOUT" and exits 0."""
import subprocess, sys, time

cmd = sys.argv[1:]
t0 = time.perf_counter()
try:
    subprocess.run(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=60)
except subprocess.TimeoutExpired:
    print("TIMEOUT")
    sys.exit(0)
elapsed_ms = (time.perf_counter() - t0) * 1000
print(f"{elapsed_ms:.0f}")
