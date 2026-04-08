#!/bin/bash
# Side-by-side benchmark B: Soma vs Python+Numba.
#
# Like bench/compare.sh but the Python side uses bench/numba/<name>.py
# (which uses @njit(cache=True) where Numba can help, plain CPython
# where Numba can't — BigInt, strings, etc.)
#
# Two metrics per challenge:
#   wall    — total wall time (process startup + JIT compile + workload)
#   inner   — workload-only timing (excludes startup AND JIT compile,
#             since Numba scripts call warmup() before the timer)
#
# Numba's @njit(cache=True) persists compiled artifacts to __pycache__,
# so the JIT compile cost is paid once and amortized across runs. The
# inner measurement is the fairest computation-only comparison.
#
# Usage:  bench/compare_B.sh             — all
#         bench/compare_B.sh foo bar     — only those

set -e
cd "$(dirname "$0")/.."

SOMA=./compiler/target/release/soma
PY=python3

# Pre-warm Soma cache so dylib is built before timing.
warm_soma() { $SOMA run "examples/$1.cell" >/dev/null 2>&1 || true; }

# Pre-warm Numba cache so JIT compile is done before timing.
warm_numba() {
  ($PY "bench/numba/$1.py" >/dev/null 2>&1 || true)
}

# Hard timeout (seconds) on any single bench invocation.
TIMEOUT=120
with_timeout() {
  $PY bench/run_with_timeout.py $TIMEOUT "$@"
}

# Read INNER_US:<N> (microseconds) from stderr, convert to ms with 2 decimals.
numba_inner_ms() {
  local us
  us=$(with_timeout $PY "bench/numba/$1.py" 2>&1 1>/dev/null | grep -oE 'INNER_US:[0-9]+' | head -1 | cut -d: -f2)
  if [ -z "$us" ]; then
    return
  fi
  awk -v u="$us" 'BEGIN { printf "%.2f", u/1000 }'
}

# Sum of (<N>ms) markers in the soma cell output (the headline + setup checks).
soma_inner_ms() {
  with_timeout $SOMA run "examples/$1.cell" 2>&1 \
    | grep -oE '\([0-9]+ms\)' \
    | grep -oE '[0-9]+' \
    | awk '{ s += $1 } END { printf "%d", s }'
}

# Wall-clock for a single subprocess invocation
wall_ms() { $PY bench/time_ms.py "$@"; }

run_one() {
  local cell=$1
  if [ ! -f "examples/$cell.cell" ] || [ ! -f "bench/numba/$cell.py" ]; then return; fi
  warm_soma "$cell"
  warm_numba "$cell"
  local soma_wall=$(wall_ms $SOMA run "examples/$cell.cell")
  local numba_wall=$(wall_ms $PY "bench/numba/$cell.py")
  local soma_inner=$(soma_inner_ms "$cell")
  local numba_inner=$(numba_inner_ms "$cell")
  if [ -z "$soma_inner" ] || [ "$soma_inner" = "0" ]; then soma_inner="—"; fi
  if [ -z "$numba_inner" ]; then numba_inner="—"; fi
  local speedup="—"
  # Both must be numeric (int or float) and positive
  if [[ "$soma_inner" != "—" ]] && [[ "$numba_inner" != "—" ]]; then
    speedup=$(awk -v p="$numba_inner" -v s="$soma_inner" \
      'BEGIN { if (s+0 > 0 && p+0 > 0) printf "%.2fx", (p+0)/(s+0); else print "—" }')
  fi
  if [ "$speedup" = "—" ] && [[ "$soma_wall" =~ ^[0-9]+$ ]] && [[ "$numba_wall" =~ ^[0-9]+$ ]] && [ "$soma_wall" -gt 0 ]; then
    speedup=$(awk -v p="$numba_wall" -v s="$soma_wall" 'BEGIN { printf "%.2fx", p/s }')"(wall)"
  fi
  printf "  %-22s  Soma: wall %5sms inner %5sms   Numba: wall %5sms inner %7sms   speedup: %12s\n" \
    "$cell" "$soma_wall" "$soma_inner" "$numba_wall" "$numba_inner" "$speedup"
}

if [ $# -gt 0 ]; then
  for c in "$@"; do run_one "$c"; done
else
  for py in bench/numba/*.py; do
    base=$(basename "$py" .py)
    [[ "$base" == _* ]] && continue
    run_one "$base"
  done
fi
