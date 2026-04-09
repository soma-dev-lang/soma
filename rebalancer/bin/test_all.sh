#!/usr/bin/env bash
# Run the entire test suite for the rebalancer.
#
#   bin/test_all.sh           # unit + CRUD + e2e mock (fast, ~5s)
#   bin/test_all.sh --live    # also include the live LLM test (~2 min)
#
# Stops on the first failing suite. Returns non-zero on any failure.
set -euo pipefail

cd "$(dirname "$0")/../.."

INCLUDE_LIVE=0
if [[ "${1:-}" == "--live" ]]; then
    INCLUDE_LIVE=1
fi

SOMA="./compiler/target/release/soma"

PASS=0
FAIL=0

run() {
    local label="$1"; shift
    echo
    echo "════════════════════════════════════════════════════════"
    echo "  $label"
    echo "════════════════════════════════════════════════════════"
    if "$@"; then
        PASS=$((PASS + 1))
    else
        FAIL=$((FAIL + 1))
        echo "FAIL: $label"
    fi
}

# 1. Static checks (compile + state-machine verification)
echo
echo "════════════════════════════════════════════════════════"
echo "  STATIC: soma check + verify"
echo "════════════════════════════════════════════════════════"
"$SOMA" check  rebalancer/app.cell
"$SOMA" verify rebalancer/app.cell | tail -25

# 2. Unit tests (Alpha + Optimizer math)
run "UNIT TESTS"  ./rebalancer/bin/test_unit.sh

# 3. CRUD HTTP tests (no LLM)
run "CRUD TESTS"  ./rebalancer/bin/test_crud.sh

# 4. End-to-end lifecycle tests (mocked LLM, fast)
run "E2E (mock)"  ./rebalancer/bin/test_e2e.sh

# 5. Live LLM test (slow, opt-in)
if [[ $INCLUDE_LIVE -eq 1 ]]; then
    run "LIVE LLM"  ./rebalancer/bin/test_live.sh
else
    echo
    echo "  (skipped LIVE LLM test — re-run with --live to include)"
fi

echo
echo "════════════════════════════════════════════════════════"
if [[ $FAIL -eq 0 ]]; then
    echo "  ALL SUITES GREEN ($PASS/$PASS)"
else
    echo "  $FAIL SUITE(S) FAILED ($PASS passed)"
fi
echo "════════════════════════════════════════════════════════"
exit $FAIL
