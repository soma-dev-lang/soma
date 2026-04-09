#!/usr/bin/env bash
# End-to-end lifecycle test — exercises the full hot path through every
# state of the rebalance state machine, with the LLM stubbed via
# SOMA_LLM_MOCK so tests are fast and deterministic.
#
#   bin/test_e2e.sh
#
# Three runs against three different mock verdicts:
#   1. APPROVE → full happy path → executed → reconciled → closed
#   2. BLOCK   → blocked at compliance → closed
#   3. FLAG    → human gate → cancel → closed
#
# Asserts on the audit trail, run state, and resulting positions.
set -euo pipefail

cd "$(dirname "$0")/../.."

SOMA="./compiler/target/release/soma"
PORT=18081
BASE="http://localhost:$PORT"
LOG="/tmp/rebalancer_e2e.log"

PASS=0
FAIL=0

ok()   { echo "  ✓ $1"; PASS=$((PASS + 1)); }
fail() { echo "  ✗ $1"; FAIL=$((FAIL + 1)); }

post() { curl -s -X POST -H 'content-type: application/json' -d "$2" "$BASE$1"; }
get()  { curl -s "$BASE$1"; }

start_server() {
    local mock="$1"
    # Wipe persistent state so each run starts clean.
    rm -rf .soma_data
    SOMA_LLM_MOCK="$mock" "$SOMA" serve rebalancer/app.cell -p "$PORT" >"$LOG" 2>&1 &
    SERVER_PID=$!
    for i in $(seq 1 30); do
        if curl -s -o /dev/null -w '%{http_code}' "$BASE/" 2>/dev/null | grep -q '^200$'; then
            return 0
        fi
        sleep 1
    done
    echo "FATAL: server did not start"
    cat "$LOG"
    exit 2
}

stop_server() {
    if [[ -n "${SERVER_PID:-}" ]]; then
        kill "$SERVER_PID" 2>/dev/null || true
        wait "$SERVER_PID" 2>/dev/null || true
        SERVER_PID=""
    fi
}
on_exit() {
    local rc=$?
    stop_server
    exit "$rc"
}
trap on_exit EXIT

setup_strategy() {
    # Strategy with momentum alpha so we exercise BOTH the alpha cell
    # and the optimizer in one rebalance.
    post /strategies '{
        "id":"e2e_strat","name":"E2E Strategy",
        "alpha":{"method":"momentum","universe":["AAPL","MSFT","GOOG","NVDA"],"top_k":"2","lookback":"5"},
        "max_position":"0.6","max_turnover":"1.0","cash_floor":"0.02","allow_shorting":"false"
    }' > /dev/null

    post /positions '{"symbol":"CASH","strategy":"e2e_strat","qty":"100000"}' > /dev/null

    post /prices '{"prices":{"AAPL":"100","MSFT":"200","GOOG":"50","NVDA":"800"}}' > /dev/null

    # Histories where NVDA dominates and AAPL is second — momentum top_k=2
    # should select NVDA + AAPL.
    post /history '{"symbol":"AAPL","prices":["80","85","90","95","100","100"]}' > /dev/null
    post /history '{"symbol":"MSFT","prices":["200","200","200","200","200","200"]}' > /dev/null
    post /history '{"symbol":"GOOG","prices":["55","54","53","52","51","50"]}' > /dev/null
    post /history '{"symbol":"NVDA","prices":["400","500","600","700","780","800"]}' > /dev/null

    post /policy '{"key":"compliance_doc","value":"Standard policy."}' > /dev/null
}

assert_eq() {
    local desc="$1"; local expected="$2"; local actual="$3"
    if [[ "$expected" == "$actual" ]]; then
        ok "$desc"
    else
        fail "$desc — expected '$expected', got '$actual'"
    fi
}

# ════════════════════════════════════════════════════════════
# RUN 1 — APPROVE path: full happy lifecycle
# ════════════════════════════════════════════════════════════
echo
echo "════════════════════════════════════════════════════════"
echo "RUN 1 — APPROVE path"
echo "════════════════════════════════════════════════════════"
start_server "fixed:VERDICT: APPROVE REASON: trades within policy NOTES: NONE"

setup_strategy
ok "setup complete"

# ── Trigger rebalance
RESP=$(post /rebalance '{"strategy_id":"e2e_strat"}')
RUN_ID=$(echo "$RESP" | jq -r '.run_id')
STATUS=$(echo "$RESP" | jq -r '.status')
VERDICT=$(echo "$RESP" | jq -r '.verdict')

[[ "$RUN_ID" == "RUN-1" ]] && ok "run_id=RUN-1" || fail "run_id=$RUN_ID"
assert_eq "rebalance status=approved" "approved" "$STATUS"
assert_eq "compliance verdict=APPROVE" "APPROVE" "$VERDICT"

N_TRADES=$(echo "$RESP" | jq -r '.n_trades')
if [[ "$N_TRADES" -ge 1 ]]; then
    ok "alpha+optimizer produced $N_TRADES trade(s)"
else
    fail "expected ≥1 trade, got $N_TRADES"
fi

# ── Snapshot must exist
RESP=$(get "/snapshot/$RUN_ID")
SNAP_NAV=$(echo "$RESP" | jq -r '.nav')
if [[ "$SNAP_NAV" == "100000.0" || "$SNAP_NAV" == "100000" ]]; then
    ok "pre-trade snapshot NAV=100000"
else
    fail "snapshot NAV=$SNAP_NAV"
fi

# ── Run should be in 'approved' state
RESP=$(get "/run/$RUN_ID")
STATUS=$(echo "$RESP" | jq -r '.status')
assert_eq "run status=approved" "approved" "$STATUS"

# ── Execute
RESP=$(post /execute "{\"run_id\":\"$RUN_ID\"}")
STATUS=$(echo "$RESP" | jq -r '.status')
N_FILLED=$(echo "$RESP" | jq -r '.filled')
assert_eq "execute status=executed" "executed" "$STATUS"
if [[ "$N_FILLED" -ge 1 ]]; then
    ok "execute filled $N_FILLED"
else
    fail "expected ≥1 fill, got $N_FILLED"
fi

# ── Positions changed: cash should be < 100000
RESP=$(get /portfolio)
CASH=$(echo "$RESP" | jq -r '.[] | select(.strategy_id=="e2e_strat") | .weights[] | select(.symbol=="CASH") | .value')
if (( $(echo "$CASH < 100000" | bc -l) )); then
    ok "CASH reduced after execute (now $CASH)"
else
    fail "CASH unchanged: $CASH"
fi

# ── Reconcile
RESP=$(post /reconcile "{\"run_id\":\"$RUN_ID\"}")
STATUS=$(echo "$RESP" | jq -r '.status')
COMMENTARY=$(echo "$RESP" | jq -r '.commentary')
assert_eq "reconcile status=closed" "closed" "$STATUS"
if [[ -n "$COMMENTARY" && "$COMMENTARY" != "null" ]]; then
    ok "commentary present"
else
    fail "commentary missing"
fi

# ── Audit trail must contain key events
RESP=$(get "/audit/$RUN_ID")
EVENTS=$(echo "$RESP" | jq -r '.[].kind' | sort -u | tr '\n' ' ')
echo "  audit events: $EVENTS"
for required in requested snapshot alpha_computed optimized compliance approved executing executed commentary closed; do
    if echo "$EVENTS" | grep -q "$required"; then
        ok "audit contains '$required'"
    else
        fail "audit missing '$required'"
    fi
done

# ── Final run state must be closed
RESP=$(get "/run/$RUN_ID")
STATUS=$(echo "$RESP" | jq -r '.status')
assert_eq "final run status=closed" "closed" "$STATUS"

stop_server
sleep 1

# ════════════════════════════════════════════════════════════
# RUN 2 — BLOCK path
# ════════════════════════════════════════════════════════════
echo
echo "════════════════════════════════════════════════════════"
echo "RUN 2 — BLOCK path"
echo "════════════════════════════════════════════════════════"
start_server "fixed:VERDICT: BLOCK REASON: policy violation simulated NOTES: test"

setup_strategy
ok "setup complete"

RESP=$(post /rebalance '{"strategy_id":"e2e_strat"}')
RUN_ID=$(echo "$RESP" | jq -r '.run_id')
STATUS=$(echo "$RESP" | jq -r '.status')
BLOCKED=$(echo "$RESP" | jq -r '.blocked')
assert_eq "rebalance status=closed (blocked)" "closed" "$STATUS"
assert_eq "blocked flag=true" "true" "$BLOCKED"

# Audit must include 'blocked'
RESP=$(get "/audit/$RUN_ID")
if echo "$RESP" | jq -r '.[].kind' | grep -q '^blocked$'; then
    ok "audit contains 'blocked'"
else
    fail "audit missing 'blocked'"
fi

# Positions must NOT have changed
RESP=$(get /portfolio)
CASH=$(echo "$RESP" | jq -r '.[] | select(.strategy_id=="e2e_strat") | .weights[] | select(.symbol=="CASH") | .value')
if [[ "$CASH" == "100000" || "$CASH" == "100000.0" ]]; then
    ok "CASH unchanged after BLOCK ($CASH)"
else
    fail "CASH changed: $CASH"
fi

stop_server
sleep 1

# ════════════════════════════════════════════════════════════
# RUN 3 — FLAG path → manual approve → execute → close
# ════════════════════════════════════════════════════════════
echo
echo "════════════════════════════════════════════════════════"
echo "RUN 3 — FLAG path → manual approve"
echo "════════════════════════════════════════════════════════"
start_server "fixed:VERDICT: FLAG REASON: human review NOTES: large pos"

setup_strategy
ok "setup complete"

RESP=$(post /rebalance '{"strategy_id":"e2e_strat"}')
RUN_ID=$(echo "$RESP" | jq -r '.run_id')
STATUS=$(echo "$RESP" | jq -r '.status')
VERDICT=$(echo "$RESP" | jq -r '.verdict')
assert_eq "rebalance status=flagged" "flagged" "$STATUS"
assert_eq "verdict=FLAG" "FLAG" "$VERDICT"

# Manual approve from flagged
RESP=$(post /approve "{\"run_id\":\"$RUN_ID\"}")
STATUS=$(echo "$RESP" | jq -r '.status')
assert_eq "approve from flagged status=approved" "approved" "$STATUS"

# After approve, the run should be in 'approved' state on read
RESP=$(get "/run/$RUN_ID")
STATUS=$(echo "$RESP" | jq -r '.status')
assert_eq "post-approve run status=approved" "approved" "$STATUS"

# Drive the rest of the lifecycle
RESP=$(post /execute "{\"run_id\":\"$RUN_ID\"}")
STATUS=$(echo "$RESP" | jq -r '.status')
assert_eq "execute status=executed" "executed" "$STATUS"

RESP=$(post /reconcile "{\"run_id\":\"$RUN_ID\"}")
STATUS=$(echo "$RESP" | jq -r '.status')
assert_eq "reconcile status=closed" "closed" "$STATUS"

stop_server
sleep 1

# ════════════════════════════════════════════════════════════
# RUN 4 — FLAG path → cancel
# ════════════════════════════════════════════════════════════
echo
echo "════════════════════════════════════════════════════════"
echo "RUN 4 — FLAG path → cancel"
echo "════════════════════════════════════════════════════════"
start_server "fixed:VERDICT: FLAG REASON: human review NOTES: large pos"

setup_strategy
ok "setup complete"

RESP=$(post /rebalance '{"strategy_id":"e2e_strat"}')
RUN_ID=$(echo "$RESP" | jq -r '.run_id')

# Cancel the flagged run
RESP=$(post /cancel "{\"run_id\":\"$RUN_ID\",\"reason\":\"e2e test cancel\"}")
STATUS=$(echo "$RESP" | jq -r '.status')
CANCELLED=$(echo "$RESP" | jq -r '.cancelled')
assert_eq "cancel status=closed" "closed" "$STATUS"
assert_eq "cancelled flag=true" "true" "$CANCELLED"

# Trying to approve a closed run should fail
RESP=$(post /approve "{\"run_id\":\"$RUN_ID\"}")
ERR=$(echo "$RESP" | jq -r '.error')
if [[ "$ERR" != "null" && -n "$ERR" ]]; then
    ok "approve on closed run rejected: $ERR"
else
    fail "approve on closed run was accepted"
fi

# Trying to execute a cancelled run should fail
RESP=$(post /execute "{\"run_id\":\"$RUN_ID\"}")
ERR=$(echo "$RESP" | jq -r '.error')
if [[ "$ERR" != "null" && -n "$ERR" ]]; then
    ok "execute on closed run rejected: $ERR"
else
    fail "execute on closed run was accepted"
fi

stop_server

# ════════════════════════════════════════════════════════════
echo
echo "════════════════════════════════════════════════════════"
echo "E2E: $PASS passed, $FAIL failed"
echo "════════════════════════════════════════════════════════"
exit $FAIL
