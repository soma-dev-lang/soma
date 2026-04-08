"""Time a subprocess to millisecond precision and print the elapsed ms."""
import subprocess, sys, time

cmd = sys.argv[1:]
t0 = time.perf_counter()
subprocess.run(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
elapsed_ms = (time.perf_counter() - t0) * 1000
print(f"{elapsed_ms:.0f}")
