#!/usr/bin/env bash
# CRUD smoke test — drives every endpoint that does NOT touch the LLM.
#
#   bin/test_crud.sh
#
# Spins up `soma serve` on a free port, hits every CRUD endpoint with
# curl, asserts on the JSON responses with jq, then shuts down.
#
# This test does NOT exercise rebalance(), execute(), or reconcile()
# because those go through the Compliance / Commentary cells which
# require ollama. Use bin/test_e2e.sh for the full lifecycle test.
set -euo pipefail

cd "$(dirname "$0")/../.."

SOMA="./compiler/target/release/soma"
PORT=18080
BASE="http://localhost:$PORT"
LOG="/tmp/rebalancer_crud.log"

PASS=0
FAIL=0

ok()   { echo "  ✓ $1"; PASS=$((PASS + 1)); }
fail() { echo "  ✗ $1"; FAIL=$((FAIL + 1)); }

# Start the server
"$SOMA" serve rebalancer/app.cell -p "$PORT" >"$LOG" 2>&1 &
SERVER_PID=$!
cleanup() {
    local rc=$?
    kill "$SERVER_PID" 2>/dev/null || true
    wait "$SERVER_PID" 2>/dev/null || true
    exit "$rc"
}
trap cleanup EXIT

# Wait for server to be ready
echo "Starting soma serve on :$PORT (pid=$SERVER_PID)..."
for i in $(seq 1 30); do
    if curl -s -o /dev/null -w '%{http_code}' "$BASE/" 2>/dev/null | grep -q '^200$'; then
        echo "  ready after ${i}s"
        break
    fi
    sleep 1
    if [[ $i -eq 30 ]]; then
        echo "FATAL: server did not become ready"
        cat "$LOG"
        exit 2
    fi
done

# Helper: post JSON, return body
post() { curl -s -X POST -H 'content-type: application/json' -d "$2" "$BASE$1"; }
get()  { curl -s "$BASE$1"; }

echo
echo "==== Index endpoint ===="
RESP=$(get /)
NAME=$(echo "$RESP" | jq -r '.name')
[[ "$NAME" == "Soma Systematic Rebalancer" ]] && ok "GET / returns correct name" || fail "GET / name=$NAME"

echo
echo "==== Strategy CRUD ===="
RESP=$(post /strategies '{"id":"test_strat","name":"Test Strategy","targets":{"AAPL":"0.5","MSFT":"0.5"},"max_position":"0.6","max_turnover":"1.0","cash_floor":"0.05","allow_shorting":"false"}')
ID=$(echo "$RESP" | jq -r '.id')
[[ "$ID" == "test_strat" ]] && ok "POST /strategies upsert" || fail "upsert id=$ID"

RESP=$(get /strategies)
COUNT=$(echo "$RESP" | jq -r 'length')
[[ "$COUNT" == "1" ]] && ok "GET /strategies count=1" || fail "GET /strategies count=$COUNT"

RESP=$(get /strategy/test_strat)
NAME=$(echo "$RESP" | jq -r '.name')
[[ "$NAME" == "Test Strategy" ]] && ok "GET /strategy/test_strat name" || fail "name=$NAME"

# Strategy with alpha block
RESP=$(post /strategies '{"id":"momo_strat","name":"Momo","alpha":{"method":"momentum","universe":["AAPL","MSFT","GOOG"],"top_k":"2","lookback":"5"},"max_position":"0.5","max_turnover":"1.0","cash_floor":"0.0","allow_shorting":"false"}')
HAS_ALPHA=$(echo "$RESP" | jq -r '.alpha')
[[ "$HAS_ALPHA" == "true" ]] && ok "POST /strategies with alpha" || fail "alpha=$HAS_ALPHA"

echo
echo "==== Position CRUD ===="
RESP=$(post /positions '{"symbol":"AAPL","strategy":"test_strat","qty":"100"}')
SYM=$(echo "$RESP" | jq -r '.symbol')
[[ "$SYM" == "AAPL" ]] && ok "POST /positions" || fail "sym=$SYM"

post /positions '{"symbol":"MSFT","strategy":"test_strat","qty":"50"}' > /dev/null
post /positions '{"symbol":"CASH","strategy":"test_strat","qty":"5000"}' > /dev/null
ok "seeded 3 positions"

# Missing-symbol error
RESP=$(post /positions '{"qty":"10","strategy":"test_strat"}')
ERR=$(echo "$RESP" | jq -r '.error')
[[ "$ERR" == "symbol required" ]] && ok "missing-symbol returns error" || fail "err=$ERR"

echo
echo "==== Prices ===="
RESP=$(post /prices '{"prices":{"AAPL":"175","MSFT":"410","GOOG":"140","NVDA":"800","AMZN":"185"}}')
N=$(echo "$RESP" | jq -r '.updated')
[[ "$N" == "5" ]] && ok "POST /prices updated 5" || fail "updated=$N"

echo
echo "==== Price history (Alpha input) ===="
post /history '{"symbol":"AAPL","prices":["100","102","104","106","108","110"]}' > /dev/null
post /history '{"symbol":"MSFT","prices":["200","204","208","212","216","220"]}' > /dev/null
post /history '{"symbol":"GOOG","prices":["100","99","98","97","96","95"]}' > /dev/null
ok "seeded 3 histories"

RESP=$(get /history/AAPL)
NPRICES=$(echo "$RESP" | jq -r '.n')
[[ "$NPRICES" == "6" ]] && ok "GET /history/AAPL n=6" || fail "n=$NPRICES"

RESP=$(get /history/UNKNOWN)
ERR=$(echo "$RESP" | jq -r '.error')
[[ "$ERR" == "no history" ]] && ok "GET /history/UNKNOWN error" || fail "err=$ERR"

echo
echo "==== Policy ===="
post /policy '{"key":"compliance_doc","value":"No single name > 60% NAV. Test policy."}' > /dev/null
post /policy '{"key":"max_loss","value":"0.05"}' > /dev/null

RESP=$(get /policy)
COUNT=$(echo "$RESP" | jq -r 'length')
[[ "$COUNT" == "2" ]] && ok "GET /policy count=2" || fail "policy count=$COUNT"

echo
echo "==== Portfolio read ===="
RESP=$(get /portfolio)
COUNT=$(echo "$RESP" | jq -r 'length')
# 2 strategies (test_strat + momo_strat)
[[ "$COUNT" == "2" ]] && ok "GET /portfolio 2 strategies" || fail "portfolio count=$COUNT"

# NAV for test_strat = 100*175 + 50*410 + 5000*1 = 17500 + 20500 + 5000 = 43000
NAV=$(echo "$RESP" | jq -r '.[] | select(.strategy_id=="test_strat") | .nav')
if (( $(echo "$NAV >= 42999 && $NAV <= 43001" | bc -l) )); then
    ok "test_strat NAV = 43000 (got $NAV)"
else
    fail "test_strat NAV expected 43000, got $NAV"
fi

# Weight check: AAPL = 17500/43000 ≈ 0.407
AAPL_W=$(echo "$RESP" | jq -r '.[] | select(.strategy_id=="test_strat") | .weights[] | select(.symbol=="AAPL") | .weight')
W_OK=$(echo "$AAPL_W > 0.40 && $AAPL_W < 0.41" | bc -l)
[[ "$W_OK" == "1" ]] && ok "AAPL weight ≈ 0.407 (got $AAPL_W)" || fail "AAPL weight=$AAPL_W"

echo
echo "==== Unknown route ===="
RESP=$(get /this_does_not_exist)
ERR=$(echo "$RESP" | jq -r '.error')
[[ "$ERR" == "not found" ]] && ok "404 returns error" || fail "err=$ERR"

echo
echo "================================================"
echo "CRUD: $PASS passed, $FAIL failed"
echo "================================================"
exit $FAIL
