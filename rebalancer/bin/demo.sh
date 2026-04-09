#!/usr/bin/env bash
# End-to-end demo of the rebalancer with realistic data.
#
#   bin/demo.sh           # uses mocked LLM (fast, deterministic)
#   bin/demo.sh --live    # uses real ollama / gemma4:26b (slow)
#
# Sets up a momentum strategy across the FAANG+ universe, marks the
# book to current prices, runs a full rebalance hot path through the
# state machine, and prints the result of every stage.
set -euo pipefail

cd "$(dirname "$0")/../.."

SOMA="./compiler/target/release/soma"
PORT=18090
BASE="http://localhost:$PORT"
LOG="/tmp/rebalancer_demo.log"

USE_LIVE=0
if [[ "${1:-}" == "--live" ]]; then USE_LIVE=1; fi

post() { curl -s -X POST -H 'content-type: application/json' -d "$2" "$BASE$1"; }
get()  { curl -s "$BASE$1"; }

step() { echo; echo "── $1 ─────────────────────────────────"; }

rm -rf .soma_data
if [[ $USE_LIVE -eq 1 ]]; then
    if ! curl -s http://localhost:11434/api/tags > /dev/null; then
        echo "ERROR: --live requires ollama at http://localhost:11434"
        exit 1
    fi
    unset SOMA_LLM_MOCK
    echo "Starting rebalancer with REAL LLM (gemma4:26b)..."
else
    export SOMA_LLM_MOCK="fixed:VERDICT: APPROVE REASON: trades within policy NOTES: NONE"
    echo "Starting rebalancer with MOCKED LLM (auto-approve)..."
fi

"$SOMA" serve rebalancer/app.cell -p "$PORT" >"$LOG" 2>&1 &
SERVER_PID=$!
cleanup() {
    local rc=$?
    kill "$SERVER_PID" 2>/dev/null || true
    wait "$SERVER_PID" 2>/dev/null || true
    exit "$rc"
}
trap cleanup EXIT

for i in $(seq 1 30); do
    if curl -s -o /dev/null -w '%{http_code}' "$BASE/" 2>/dev/null | grep -q '^200$'; then
        echo "  ready (pid $SERVER_PID, port $PORT)"
        break
    fi
    sleep 1
done

# ── 1. Define a momentum strategy ─────────────────────────
step "1. Define strategy: 'FAANG+ Momentum Top-3'"

post /strategies '{
    "id": "faang_momo",
    "name": "FAANG+ Momentum Top-3",
    "alpha": {
        "method": "momentum",
        "universe": ["AAPL", "MSFT", "GOOG", "AMZN", "META", "NVDA", "TSLA"],
        "top_k": "3",
        "lookback": "20"
    },
    "max_position": "0.40",
    "max_turnover": "1.00",
    "cash_floor": "0.05",
    "allow_shorting": "false"
}' | jq .

# ── 2. Seed positions: start with $5M cash ────────────────
step "2. Seed positions: \$5M cash, no holdings"

post /positions '{"symbol":"CASH","strategy":"faang_momo","qty":"5000000"}' | jq .

# ── 3. Mark to market ──────────────────────────────────────
step "3. Mark to market (today's prices)"

post /prices '{
    "prices": {
        "AAPL": "190",
        "MSFT": "420",
        "GOOG": "155",
        "AMZN": "195",
        "META":  "510",
        "NVDA": "1180",
        "TSLA": "175"
    }
}' | jq .

# ── 4. Seed price history for momentum (30 days each) ─────
step "4. Seed 30-day price histories for momentum signal"

# Made-up but plausible 30-day series. NVDA has the strongest momentum,
# META is second, AAPL third. AMZN is flat. TSLA is the worst (declining).
post /history '{"symbol":"NVDA","prices":["800","820","850","870","880","900","920","940","950","970","990","1010","1030","1050","1070","1080","1090","1100","1110","1120","1130","1140","1150","1155","1160","1165","1170","1175","1178","1180"]}' > /dev/null
post /history '{"symbol":"META","prices":["410","415","420","425","430","435","440","445","450","455","460","465","470","475","478","480","485","488","490","492","495","498","500","502","504","506","508","509","510","510"]}' > /dev/null
post /history '{"symbol":"AAPL","prices":["170","171","172","173","174","175","176","177","178","179","180","181","182","183","184","185","186","187","188","188","189","189","189","190","190","190","190","190","190","190"]}' > /dev/null
post /history '{"symbol":"MSFT","prices":["410","411","412","413","414","415","416","417","418","419","420","420","421","421","420","420","419","420","420","420","420","420","420","420","420","420","420","420","420","420"]}' > /dev/null
post /history '{"symbol":"GOOG","prices":["150","151","152","153","154","155","156","156","155","155","154","154","153","153","154","154","155","155","154","154","155","155","155","155","155","155","155","155","155","155"]}' > /dev/null
post /history '{"symbol":"AMZN","prices":["195","195","195","195","195","195","195","195","195","195","195","195","195","195","195","195","195","195","195","195","195","195","195","195","195","195","195","195","195","195"]}' > /dev/null
post /history '{"symbol":"TSLA","prices":["220","218","216","214","212","210","208","206","204","202","200","198","196","194","192","190","188","186","184","183","182","181","180","179","178","177","176","176","175","175"]}' > /dev/null
echo '  seeded 7 histories'

# ── 5. Set firm policy (LLM compliance reads this) ────────
step "5. Set firm compliance policy"

post /policy '{"key":"compliance_doc","value":"Position concentration limit: 40% of NAV per single name. Daily turnover cap: 100% of NAV. No naked shorts. Momentum strategies must trade within S&P 500 universe. All trades > $1M flagged for human review."}' | jq .

# ── 6. THE HOT PATH ────────────────────────────────────────
step "6. POST /rebalance — alpha → optimize → compliance"

REBAL=$(post /rebalance '{"strategy_id":"faang_momo"}')
echo "$REBAL" | jq .

RUN_ID=$(echo "$REBAL" | jq -r '.run_id')
STATUS=$(echo "$REBAL" | jq -r '.status')

# ── 7. Pre-trade snapshot (proves auditability) ───────────
step "7. GET /snapshot/$RUN_ID — pre-trade weight snapshot"

get "/snapshot/$RUN_ID" | jq .

# ── 8. Trade list ──────────────────────────────────────────
step "8. GET /trades/$RUN_ID — proposed trades"

get "/trades/$RUN_ID" | jq .

# ── 9. Approve if flagged ──────────────────────────────────
if [[ "$STATUS" == "flagged" ]]; then
    step "9. POST /approve — manual gate"
    post /approve "{\"run_id\":\"$RUN_ID\"}" | jq .
elif [[ "$STATUS" == "approved" ]]; then
    step "9. (auto-approved by compliance, no manual gate needed)"
fi

# ── 10. Execute (sim fills) ────────────────────────────────
step "10. POST /execute — fills against current marks"

post /execute "{\"run_id\":\"$RUN_ID\"}" | jq .

# ── 11. Reconcile (post-trade commentary) ──────────────────
step "11. POST /reconcile — commentary + close"

RECON=$(post /reconcile "{\"run_id\":\"$RUN_ID\"}")
echo "$RECON" | jq .

# ── 12. Final portfolio state ──────────────────────────────
step "12. GET /portfolio — post-trade state"

get /portfolio | jq .

# ── 13. Full audit trail ───────────────────────────────────
step "13. GET /audit/$RUN_ID — full event log"

get "/audit/$RUN_ID" | jq -r '.[] | "  \(.ts)  \(.kind): \(.detail)"' | head -30

echo
echo "════════════════════════════════════════════════════════"
echo "  Demo complete — run $RUN_ID closed"
echo "════════════════════════════════════════════════════════"
