#!/bin/bash
# Side-by-side benchmark: Soma vs Python.
#
# Two metrics per challenge:
#   wall    — total wall time (process startup + workload)
#   inner   — workload-only timing (excludes process startup)
#
# Soma "inner" = the headline-style "(<N>ms)" that the cell prints.
# Python "inner" = the cell's Python equivalent prints "INNER:<N>" on stderr.
#
# Wall = python bench/time_ms.py for both Soma and Python invocations.
#
# Usage:  bench/compare.sh             — all
#         bench/compare.sh foo bar     — only those

set -e
cd "$(dirname "$0")/.."

SOMA=./compiler/target/release/soma
PY=python3

# Pre-warm Soma cache so dylib is built before timing.
warm_soma() { $SOMA run "examples/$1.cell" >/dev/null 2>&1 || true; }

# Hard timeout (seconds) on any single bench invocation. Implemented via
# a Python wrapper because macOS doesn't ship `timeout` and bash signal
# handling around backgrounded subprocesses is fragile.
TIMEOUT=60
with_timeout() {
  $PY bench/run_with_timeout.py $TIMEOUT "$@"
}

# Read INNER:<N> from stderr of the python script.
py_inner_ms() {
  with_timeout $PY "bench/py/$1.py" 2>&1 1>/dev/null | grep -oE 'INNER:[0-9]+' | head -1 | cut -d: -f2
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
  if [ ! -f "examples/$cell.cell" ] || [ ! -f "bench/py/$cell.py" ]; then return; fi
  warm_soma "$cell"
  local soma_wall=$(wall_ms $SOMA run "examples/$cell.cell")
  local py_wall=$(wall_ms $PY "bench/py/$cell.py")
  local soma_inner=$(soma_inner_ms "$cell")
  local py_inner=$(py_inner_ms "$cell")
  # Default missing inner to "—"
  if [ -z "$soma_inner" ] || [ "$soma_inner" = "0" ]; then soma_inner="—"; fi
  if [ -z "$py_inner" ]; then py_inner="—"; fi
  # Speedup based on inner timing if BOTH are numeric AND non-zero;
  # otherwise fall back to wall timing.
  local speedup="—"
  if [[ "$soma_inner" =~ ^[0-9]+$ ]] && [[ "$py_inner" =~ ^[0-9]+$ ]] \
     && [ "$soma_inner" -gt 0 ] && [ "$py_inner" -gt 0 ]; then
    speedup=$(awk -v p="$py_inner" -v s="$soma_inner" 'BEGIN { printf "%.1fx", p/s }')
  elif [[ "$soma_wall" =~ ^[0-9]+$ ]] && [[ "$py_wall" =~ ^[0-9]+$ ]] && [ "$soma_wall" -gt 0 ]; then
    speedup=$(awk -v p="$py_wall" -v s="$soma_wall" 'BEGIN { printf "%.1fx", p/s }')
  fi
  printf "  %-18s  Soma: wall %5sms inner %5sms   Python: wall %5sms inner %5sms   speedup: %7s\n" \
    "$cell" "$soma_wall" "$soma_inner" "$py_wall" "$py_inner" "$speedup"
}

if [ $# -gt 0 ]; then
  for c in "$@"; do run_one "$c"; done
else
  for py in bench/py/*.py; do
    run_one "$(basename "$py" .py)"
  done
fi
