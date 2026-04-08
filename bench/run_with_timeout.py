"""Run a subprocess with a hard timeout. Forward stdout, exit 0 on timeout.

Usage: python3 bench/run_with_timeout.py <seconds> <cmd> [args...]
"""
import subprocess, sys

if len(sys.argv) < 3:
    sys.exit(2)
timeout = int(sys.argv[1])
cmd = sys.argv[2:]
try:
    result = subprocess.run(cmd, capture_output=True, timeout=timeout)
    sys.stdout.buffer.write(result.stdout)
    sys.stderr.buffer.write(result.stderr)
except subprocess.TimeoutExpired:
    print("TIMEOUT", file=sys.stderr)
sys.exit(0)
