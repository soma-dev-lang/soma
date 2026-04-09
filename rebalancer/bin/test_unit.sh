#!/usr/bin/env bash
# Run all unit tests for the rebalancer.
#
#   bin/test_unit.sh
#
# Exits non-zero if any unit test fails.
set -euo pipefail

cd "$(dirname "$0")/../.."

SOMA="./compiler/target/release/soma"
if [[ ! -x "$SOMA" ]]; then
    echo "FATAL: $SOMA not found — build it with 'cd compiler && cargo build --release'"
    exit 1
fi

PASS=0
FAIL=0

run_suite() {
    local name="$1"
    local file="$2"
    echo
    echo "==== $name ===="
    if "$SOMA" test "$file" 2>&1; then
        PASS=$((PASS + 1))
    else
        FAIL=$((FAIL + 1))
    fi
}

run_suite "Alpha"     rebalancer/tests/test_alpha.cell
run_suite "Optimizer" rebalancer/tests/test_optimizer.cell

echo
echo "================================================"
echo "Unit suites: $PASS passed, $FAIL failed"
echo "================================================"
exit $FAIL
