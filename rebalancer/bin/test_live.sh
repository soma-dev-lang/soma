#!/usr/bin/env bash
# Live LLM test — exercises the rebalancer against a REAL ollama
# instance running gemma4:26b. Slow (~2-5 min). Skipped by test_all.sh
# unless explicitly invoked.
#
#   bin/test_live.sh
#
# Requires:
#   - ollama running on http://localhost:11434
#   - gemma4:26b model pulled (ollama pull gemma4:26b)
#
# Asserts only that the lifecycle COMPLETES, not what verdict the
# LLM gave — that depends on the model. The point of this test is
# to prove the HTTP wiring to ollama works and the parser is robust
# to real model output.
set -euo pipefail

cd "$(dirname "$0")/../.."

SOMA="./compiler/target/release/soma"
PORT=18083
BASE="http://localhost:$PORT"
LOG="/tmp/rebalancer_live.log"

# Pre-flight checks
if ! curl -s http://localhost:11434/api/tags > /dev/null; then
    echo "SKIP: ollama not reachable at http://localhost:11434"
    echo "      start ollama and pull gemma4:26b to run this test"
    exit 0
fi

if ! curl -s http://localhost:11434/api/tags | grep -q 'gemma4:26b'; then
    echo "SKIP: gemma4:26b not loaded in ollama"
    echo "      run: ollama pull gemma4:26b"
    exit 0
fi

PASS=0
FAIL=0
ok()   { echo "  ✓ $1"; PASS=$((PASS + 1)); }
fail() { echo "  ✗ $1"; FAIL=$((FAIL + 1)); }
post() { curl -s -X POST -H 'content-type: application/json' -d "$2" "$BASE$1"; }
get()  { curl -s "$BASE$1"; }

# Wipe state and start with REAL LLM (no mock)
rm -rf .soma_data
unset SOMA_LLM_MOCK
"$SOMA" serve rebalancer/app.cell -p "$PORT" >"$LOG" 2>&1 &
SERVER_PID=$!
cleanup() {
    local rc=$?
    kill "$SERVER_PID" 2>/dev/null || true
    wait "$SERVER_PID" 2>/dev/null || true
    exit "$rc"
}
trap cleanup EXIT

echo "Starting soma serve on :$PORT (live LLM mode)..."
for i in $(seq 1 30); do
    if curl -s -o /dev/null -w '%{http_code}' "$BASE/" 2>/dev/null | grep -q '^200$'; then
        echo "  ready after ${i}s"
        break
    fi
    sleep 1
done

# Setup a small strategy — fewer names to keep the LLM prompt short
post /strategies '{
    "id":"live_strat","name":"Live Test",
    "alpha":{"method":"momentum","universe":["AAPL","MSFT"],"top_k":"1","lookback":"5"},
    "max_position":"0.6","max_turnover":"1.0","cash_floor":"0.05","allow_shorting":"false"
}' > /dev/null

post /positions '{"symbol":"CASH","strategy":"live_strat","qty":"50000"}' > /dev/null
post /prices '{"prices":{"AAPL":"100","MSFT":"200"}}' > /dev/null
post /history '{"symbol":"AAPL","prices":["80","85","90","95","100"]}' > /dev/null
post /history '{"symbol":"MSFT","prices":["200","200","200","200","200"]}' > /dev/null
post /policy '{"key":"compliance_doc","value":"Standard buy-side policy. No single name > 60% NAV."}' > /dev/null
ok "setup complete"

# Run the rebalance — this is the slow part because Compliance.review
# calls the LLM
echo "  → calling LLM (this can take 30-60s)..."
START=$(date +%s)
RESP=$(post /rebalance '{"strategy_id":"live_strat"}')
END=$(date +%s)
DURATION=$((END - START))
echo "  → LLM call took ${DURATION}s"

RUN_ID=$(echo "$RESP" | jq -r '.run_id')
STATUS=$(echo "$RESP" | jq -r '.status')

if [[ "$RUN_ID" =~ ^RUN- ]]; then
    ok "run_id=$RUN_ID"
else
    fail "no run_id: $RESP"
fi

# Status should be one of approved, flagged, closed (any is fine —
# the test only proves the LLM call returned and was parsed)
case "$STATUS" in
    approved|flagged|closed)
        ok "real LLM produced parseable verdict (status=$STATUS)"
        ;;
    *)
        fail "unexpected status from real LLM: $STATUS"
        ;;
esac

# Inspect what verdict the model gave
VERDICT=$(echo "$RESP" | jq -r '.verdict // "(none)"')
echo "  → model verdict: $VERDICT"

# Drive to closed regardless of verdict
case "$STATUS" in
    flagged)
        APPROVE_RESP=$(post /approve "{\"run_id\":\"$RUN_ID\"}")
        echo "  approve response: $APPROVE_RESP"
        APPROVE_STATUS=$(echo "$APPROVE_RESP" | jq -r '.status // "(missing)"')
        if [[ "$APPROVE_STATUS" == "approved" ]]; then
            ok "manual approve from flagged"
        else
            fail "approve returned status=$APPROVE_STATUS"
        fi
        ;;
    approved)
        ok "auto-approved by LLM"
        ;;
esac

# Get current run status
CURRENT=$(get "/run/$RUN_ID" | jq -r '.status')
if [[ "$CURRENT" == "approved" || "$CURRENT" == "closed" ]]; then
    ok "run reached approved or closed (got $CURRENT)"
else
    fail "run in unexpected state: $CURRENT"
fi

# If approved (not blocked), execute and reconcile
if [[ "$CURRENT" == "approved" ]]; then
    RESP=$(post /execute "{\"run_id\":\"$RUN_ID\"}")
    EXECSTAT=$(echo "$RESP" | jq -r '.status')
    [[ "$EXECSTAT" == "executed" ]] && ok "execute → executed" || fail "execute=$EXECSTAT"

    echo "  → calling LLM for commentary (~30-60s)..."
    START=$(date +%s)
    RESP=$(post /reconcile "{\"run_id\":\"$RUN_ID\"}")
    END=$(date +%s)
    echo "  → commentary took $((END - START))s"
    RECONSTAT=$(echo "$RESP" | jq -r '.status')
    [[ "$RECONSTAT" == "closed" ]] && ok "reconcile → closed" || fail "reconcile=$RECONSTAT"

    # Print the actual commentary so we can eyeball it
    COMMENTARY=$(echo "$RESP" | jq -r '.commentary')
    echo
    echo "  --- LLM commentary ---"
    echo "$COMMENTARY" | sed 's/^/  | /'
    echo "  ---"
fi

# Print the audit trail so we have a human-readable record
echo
echo "  --- audit trail ---"
get "/audit/$RUN_ID" | jq -r '.[] | "  | \(.kind): \(.detail | tostring | .[0:80])"'
echo "  ---"

echo
echo "════════════════════════════════════════════════════════"
echo "LIVE: $PASS passed, $FAIL failed (LLM=gemma4:26b)"
echo "════════════════════════════════════════════════════════"
exit $FAIL
