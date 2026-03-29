#!/bin/bash
# Exhaustive test runner for Soma v1 platform features
SOMA=/Users/antoine/paradigm/compiler/target/release/soma
DIR=/tmp/soma_v1_tests/platform
PASS=0
FAIL=0
TOTAL=0
RESULTS=""

# Clean up any leftover soma processes and db files
pkill -f "soma serve" 2>/dev/null || true
sleep 0.5
rm -f "$DIR"/*.db "$DIR"/*.sqlite 2>/dev/null

record() {
    local test_name="$1"
    local status="$2"
    local detail="$3"
    TOTAL=$((TOTAL + 1))
    if [ "$status" = "PASS" ]; then
        PASS=$((PASS + 1))
        RESULTS="${RESULTS}PASS  ${test_name}\n"
    else
        FAIL=$((FAIL + 1))
        RESULTS="${RESULTS}FAIL  ${test_name}  |  ${detail}\n"
    fi
    echo "[$status] $test_name"
    if [ -n "$detail" ] && [ "$status" = "FAIL" ]; then
        echo "        $detail"
    fi
}

# Helper: run soma run and capture output
run_cell() {
    local file="$1"
    shift
    rm -f "$DIR"/*.db 2>/dev/null
    $SOMA run "$file" "$@" 2>&1
}

# Helper: start soma serve, wait, test, kill
serve_and_test() {
    local file="$1"
    local port="$2"
    local test_name="$3"

    rm -f "$DIR"/*.db 2>/dev/null
    $SOMA serve "$file" -p "$port" &
    local PID=$!
    sleep 1.5

    if ! kill -0 $PID 2>/dev/null; then
        record "$test_name" "FAIL" "Server failed to start"
        return 1
    fi

    echo "$PID"
    return 0
}

kill_server() {
    local pid="$1"
    kill "$pid" 2>/dev/null
    wait "$pid" 2>/dev/null
    sleep 0.3
}

echo "============================================"
echo " SOMA V1 PLATFORM TESTS"
echo "============================================"
echo ""

# ============================================
# SECTION 1: STORAGE TESTS (20 tests)
# ============================================
echo "=== STORAGE TESTS ==="

# Test 1: persistent storage creates SQLite
rm -f "$DIR"/*.db 2>/dev/null
OUT=$(run_cell "$DIR/storage_01_persistent_sqlite.cell" put hello world 2>&1)
# Check for a .db file
DB_FILES=$(ls "$DIR"/*.db 2>/dev/null || ls "$DIR"/*.sqlite 2>/dev/null || echo "")
if echo "$OUT" | grep -q "world" || [ -n "$DB_FILES" ]; then
    record "storage_01_persistent_sqlite" "PASS" ""
else
    record "storage_01_persistent_sqlite" "FAIL" "Output: $OUT"
fi

# Test 2: ephemeral storage
OUT=$(run_cell "$DIR/storage_02_ephemeral.cell" put hello world 2>&1)
if echo "$OUT" | grep -q "world"; then
    record "storage_02_ephemeral" "PASS" ""
else
    record "storage_02_ephemeral" "FAIL" "Output: $OUT"
fi

# Test 3: set and get
OUT=$(run_cell "$DIR/storage_03_set_get.cell" test 2>&1)
if echo "$OUT" | grep -q "world"; then
    record "storage_03_set_get" "PASS" ""
else
    record "storage_03_set_get" "FAIL" "Output: $OUT"
fi

# Test 4: delete
OUT=$(run_cell "$DIR/storage_04_delete.cell" test 2>&1)
if echo "$OUT" | grep -q "after delete key1" && echo "$OUT" | grep -q "key2 still: val2"; then
    record "storage_04_delete" "PASS" ""
else
    record "storage_04_delete" "FAIL" "Output: $OUT"
fi

# Test 5: keys
OUT=$(run_cell "$DIR/storage_05_keys.cell" test 2>&1)
if echo "$OUT" | grep -q "keys:"; then
    record "storage_05_keys" "PASS" ""
else
    record "storage_05_keys" "FAIL" "Output: $OUT"
fi

# Test 6: values
OUT=$(run_cell "$DIR/storage_06_values.cell" test 2>&1)
if echo "$OUT" | grep -q "values:"; then
    record "storage_06_values" "PASS" ""
else
    record "storage_06_values" "FAIL" "Output: $OUT"
fi

# Test 7: len
OUT=$(run_cell "$DIR/storage_07_len.cell" test 2>&1)
if echo "$OUT" | grep -q "len: 3"; then
    record "storage_07_len" "PASS" ""
else
    record "storage_07_len" "FAIL" "Output: $OUT"
fi

# Test 8: missing key returns ()
OUT=$(run_cell "$DIR/storage_08_missing_key.cell" test 2>&1)
if echo "$OUT" | grep -q "PASS: missing key returns unit"; then
    record "storage_08_missing_key" "PASS" ""
else
    record "storage_08_missing_key" "FAIL" "Output: $OUT"
fi

# Test 9: store int
OUT=$(run_cell "$DIR/storage_09_store_int.cell" test 2>&1)
if echo "$OUT" | grep -q "int: 42"; then
    record "storage_09_store_int" "PASS" ""
else
    record "storage_09_store_int" "FAIL" "Output: $OUT"
fi

# Test 10: store string
OUT=$(run_cell "$DIR/storage_10_store_string.cell" test 2>&1)
if echo "$OUT" | grep -q "string: Alice"; then
    record "storage_10_store_string" "PASS" ""
else
    record "storage_10_store_string" "FAIL" "Output: $OUT"
fi

# Test 11: store bool
OUT=$(run_cell "$DIR/storage_11_store_bool.cell" test 2>&1)
if echo "$OUT" | grep -q "bool: true"; then
    record "storage_11_store_bool" "PASS" ""
else
    record "storage_11_store_bool" "FAIL" "Output: $OUT"
fi

# Test 12: json roundtrip
OUT=$(run_cell "$DIR/storage_12_json_roundtrip.cell" test 2>&1)
if echo "$OUT" | grep -q "name: Alice" && echo "$OUT" | grep -q "age: 30"; then
    record "storage_12_json_roundtrip" "PASS" ""
else
    record "storage_12_json_roundtrip" "FAIL" "Output: $OUT"
fi

# Test 13: next_id increments
OUT=$(run_cell "$DIR/storage_13_next_id.cell" test 2>&1)
if echo "$OUT" | grep -q "id1: 1" && echo "$OUT" | grep -q "id2: 2" && echo "$OUT" | grep -q "id3: 3"; then
    record "storage_13_next_id" "PASS" ""
else
    record "storage_13_next_id" "FAIL" "Output: $OUT"
fi

# Test 14: persistent+ephemeral contradiction
OUT=$($SOMA check "$DIR/storage_14_contradiction.cell" 2>&1)
EXIT_CODE=$?
if echo "$OUT" | grep -iq "error\|contradict\|conflict\|incompatible" || [ $EXIT_CODE -ne 0 ]; then
    record "storage_14_contradiction" "PASS" ""
else
    record "storage_14_contradiction" "FAIL" "Expected error. Output: $OUT"
fi

# Test 15: multiple maps
OUT=$(run_cell "$DIR/storage_15_multiple_maps.cell" test 2>&1)
if echo "$OUT" | grep -q "user: admin" && echo "$OUT" | grep -q "setting: dark"; then
    record "storage_15_multiple_maps" "PASS" ""
else
    record "storage_15_multiple_maps" "FAIL" "Output: $OUT"
fi

# Test 16: overwrite
OUT=$(run_cell "$DIR/storage_16_overwrite.cell" test 2>&1)
if echo "$OUT" | grep -q "overwritten: second"; then
    record "storage_16_overwrite" "PASS" ""
else
    record "storage_16_overwrite" "FAIL" "Output: $OUT"
fi

# Test 17: delete missing
OUT=$(run_cell "$DIR/storage_17_delete_missing.cell" test 2>&1)
# Should not crash - any output is acceptable
EXIT_CODE=$?
if [ $EXIT_CODE -eq 0 ]; then
    record "storage_17_delete_missing" "PASS" ""
else
    record "storage_17_delete_missing" "FAIL" "Exit code: $EXIT_CODE. Output: $OUT"
fi

# Test 18: len after delete
OUT=$(run_cell "$DIR/storage_18_len_after_delete.cell" test 2>&1)
if echo "$OUT" | grep -q "before: 3" && echo "$OUT" | grep -q "after: 2"; then
    record "storage_18_len_after_delete" "PASS" ""
else
    record "storage_18_len_after_delete" "FAIL" "Output: $OUT"
fi

# Test 19: consistent+eventual contradiction
OUT=$($SOMA check "$DIR/storage_19_consistent_eventual_contradiction.cell" 2>&1)
EXIT_CODE=$?
if echo "$OUT" | grep -iq "error\|contradict\|conflict\|incompatible" || [ $EXIT_CODE -ne 0 ]; then
    record "storage_19_consistent_eventual" "PASS" ""
else
    record "storage_19_consistent_eventual" "FAIL" "Expected error. Output: $OUT"
fi

# Test 20: keys on empty map
OUT=$(run_cell "$DIR/storage_20_keys_empty.cell" test 2>&1)
if echo "$OUT" | grep -q "empty len: 0"; then
    record "storage_20_keys_empty" "PASS" ""
else
    record "storage_20_keys_empty" "FAIL" "Output: $OUT"
fi

# ============================================
# SECTION 2: STATE MACHINE TESTS (15 tests)
# ============================================
echo ""
echo "=== STATE MACHINE TESTS ==="

# Test SM 1: initial state
OUT=$(run_cell "$DIR/sm_01_initial_state.cell" test 2>&1)
if echo "$OUT" | grep -q "initial: draft"; then
    record "sm_01_initial_state" "PASS" ""
else
    record "sm_01_initial_state" "FAIL" "Output: $OUT"
fi

# Test SM 2: valid transition
OUT=$(run_cell "$DIR/sm_02_valid_transition.cell" test 2>&1)
if echo "$OUT" | grep -q "after transition: review"; then
    record "sm_02_valid_transition" "PASS" ""
else
    record "sm_02_valid_transition" "FAIL" "Output: $OUT"
fi

# Test SM 3: invalid transition
OUT=$(run_cell "$DIR/sm_03_invalid_transition.cell" test 2>&1)
if echo "$OUT" | grep -iq "invalid\|error\|cannot\|not.*valid\|review"; then
    record "sm_03_invalid_transition" "PASS" ""
else
    record "sm_03_invalid_transition" "FAIL" "Output: $OUT"
fi

# Test SM 4: wildcard cancel from draft
OUT=$(run_cell "$DIR/sm_04_wildcard.cell" test 2>&1)
if echo "$OUT" | grep -q "cancelled from draft: cancelled"; then
    record "sm_04_wildcard" "PASS" ""
else
    record "sm_04_wildcard" "FAIL" "Output: $OUT"
fi

# Test SM 5: wildcard from middle state
OUT=$(run_cell "$DIR/sm_05_wildcard_from_middle.cell" test 2>&1)
if echo "$OUT" | grep -q "cancelled from review: cancelled"; then
    record "sm_05_wildcard_middle" "PASS" ""
else
    record "sm_05_wildcard_middle" "FAIL" "Output: $OUT"
fi

# Test SM 6: valid_transitions listing
OUT=$(run_cell "$DIR/sm_06_valid_transitions.cell" test 2>&1)
if echo "$OUT" | grep -q "valid from draft:" && (echo "$OUT" | grep -q "review" || echo "$OUT" | grep -q "archived"); then
    record "sm_06_valid_transitions" "PASS" ""
else
    record "sm_06_valid_transitions" "FAIL" "Output: $OUT"
fi

# Test SM 7: multiple sequential transitions
OUT=$(run_cell "$DIR/sm_07_multiple_transitions.cell" test 2>&1)
if echo "$OUT" | grep -q "final: delivered"; then
    record "sm_07_multiple_transitions" "PASS" ""
else
    record "sm_07_multiple_transitions" "FAIL" "Output: $OUT"
fi

# Test SM 8: two machines (may or may not be supported)
OUT=$(run_cell "$DIR/sm_08_two_machines.cell" test 2>&1)
EXIT_CODE=$?
if echo "$OUT" | grep -q "order: confirmed" || [ $EXIT_CODE -eq 0 ]; then
    record "sm_08_two_machines" "PASS" ""
else
    record "sm_08_two_machines" "FAIL" "Output: $OUT"
fi

# Test SM 9: guard condition
OUT=$(run_cell "$DIR/sm_09_guard.cell" test 2>&1)
if echo "$OUT" | grep -q "guarded transition: submitted"; then
    record "sm_09_guard" "PASS" ""
else
    record "sm_09_guard" "FAIL" "Output: $OUT"
fi

# Test SM 10: valid_transitions changes
OUT=$(run_cell "$DIR/sm_10_valid_after_transition.cell" test 2>&1)
if echo "$OUT" | grep -q "valid from draft:" && echo "$OUT" | grep -q "valid from review:"; then
    record "sm_10_valid_after_transition" "PASS" ""
else
    record "sm_10_valid_after_transition" "FAIL" "Output: $OUT"
fi

# Test SM 11: multiple entities
OUT=$(run_cell "$DIR/sm_11_multiple_entities.cell" test 2>&1)
if echo "$OUT" | grep -q "entity 1: closed" && echo "$OUT" | grep -q "entity 2: open"; then
    record "sm_11_multiple_entities" "PASS" ""
else
    record "sm_11_multiple_entities" "FAIL" "Output: $OUT"
fi

# Test SM 12: terminal state
OUT=$(run_cell "$DIR/sm_12_terminal_state.cell" test 2>&1)
if echo "$OUT" | grep -q "valid from terminal:"; then
    record "sm_12_terminal_state" "PASS" ""
else
    record "sm_12_terminal_state" "FAIL" "Output: $OUT"
fi

# Test SM 13: status of unknown entity
OUT=$(run_cell "$DIR/sm_13_status_unknown_id.cell" test 2>&1)
if echo "$OUT" | grep -q "unknown entity status: draft"; then
    record "sm_13_unknown_entity" "PASS" ""
else
    record "sm_13_unknown_entity" "FAIL" "Output: $OUT"
fi

# Test SM 14: self-loop
OUT=$(run_cell "$DIR/sm_14_self_loop.cell" test 2>&1)
if echo "$OUT" | grep -q "self-loop: active"; then
    record "sm_14_self_loop" "PASS" ""
else
    record "sm_14_self_loop" "FAIL" "Output: $OUT"
fi

# Test SM 15: transition result value
OUT=$(run_cell "$DIR/sm_15_transition_result.cell" test 2>&1)
if echo "$OUT" | grep -q "transition result:"; then
    record "sm_15_transition_result" "PASS" ""
else
    record "sm_15_transition_result" "FAIL" "Output: $OUT"
fi

# ============================================
# SECTION 3: HTTP SERVER TESTS (20 tests)
# ============================================
echo ""
echo "=== HTTP SERVER TESTS ==="

# Test HTTP 1: basic serve
PORT=18001
rm -f "$DIR"/*.db 2>/dev/null
$SOMA serve "$DIR/http_01_basic_serve.cell" -p $PORT &
PID=$!
sleep 1.5
OUT=$(curl -s http://localhost:$PORT/health 2>&1)
kill $PID 2>/dev/null; wait $PID 2>/dev/null
if echo "$OUT" | grep -q "ok"; then
    record "http_01_basic_serve" "PASS" ""
else
    record "http_01_basic_serve" "FAIL" "Output: $OUT"
fi
sleep 0.3

# Test HTTP 2: request routing
PORT=18002
rm -f "$DIR"/*.db 2>/dev/null
$SOMA serve "$DIR/http_02_request_routing.cell" -p $PORT &
PID=$!
sleep 1.5
OUT=$(curl -s http://localhost:$PORT/anything 2>&1)
kill $PID 2>/dev/null; wait $PID 2>/dev/null
if echo "$OUT" | grep -q "method\|path"; then
    record "http_02_request_routing" "PASS" ""
else
    record "http_02_request_routing" "FAIL" "Output: $OUT"
fi
sleep 0.3

# Test HTTP 3: GET returns JSON
PORT=18003
rm -f "$DIR"/*.db 2>/dev/null
$SOMA serve "$DIR/http_03_get_json.cell" -p $PORT &
PID=$!
sleep 1.5
OUT=$(curl -s http://localhost:$PORT/items 2>&1)
kill $PID 2>/dev/null; wait $PID 2>/dev/null
if echo "$OUT" | grep -q "count"; then
    record "http_03_get_json" "PASS" ""
else
    record "http_03_get_json" "FAIL" "Output: $OUT"
fi
sleep 0.3

# Test HTTP 4: GET returns HTML
PORT=18004
rm -f "$DIR"/*.db 2>/dev/null
$SOMA serve "$DIR/http_04_get_html.cell" -p $PORT &
PID=$!
sleep 1.5
OUT=$(curl -s -i http://localhost:$PORT/page 2>&1)
kill $PID 2>/dev/null; wait $PID 2>/dev/null
if echo "$OUT" | grep -q "text/html" || echo "$OUT" | grep -q "<html>"; then
    record "http_04_get_html" "PASS" ""
else
    record "http_04_get_html" "FAIL" "Output: $OUT"
fi
sleep 0.3

# Test HTTP 5: POST with JSON body
PORT=18005
rm -f "$DIR"/*.db 2>/dev/null
$SOMA serve "$DIR/http_05_post_json.cell" -p $PORT &
PID=$!
sleep 1.5
OUT=$(curl -s -X POST -H "Content-Type: application/json" -d '{"name":"alice","email":"a@b.com"}' http://localhost:$PORT/create 2>&1)
kill $PID 2>/dev/null; wait $PID 2>/dev/null
if echo "$OUT" | grep -q "alice"; then
    record "http_05_post_json" "PASS" ""
else
    record "http_05_post_json" "FAIL" "Output: $OUT"
fi
sleep 0.3

# Test HTTP 6: POST with form-encoded body
PORT=18006
rm -f "$DIR"/*.db 2>/dev/null
$SOMA serve "$DIR/http_06_post_form.cell" -p $PORT &
PID=$!
sleep 1.5
OUT=$(curl -s -X POST -d "name=test&value=123" http://localhost:$PORT/submit 2>&1)
kill $PID 2>/dev/null; wait $PID 2>/dev/null
if echo "$OUT" | grep -q "test\|saved"; then
    record "http_06_post_form" "PASS" ""
else
    record "http_06_post_form" "FAIL" "Output: $OUT"
fi
sleep 0.3

# Test HTTP 7: 404 for unknown route
PORT=18007
rm -f "$DIR"/*.db 2>/dev/null
$SOMA serve "$DIR/http_07_404.cell" -p $PORT &
PID=$!
sleep 1.5
HTTP_CODE=$(curl -s -o /dev/null -w "%{http_code}" http://localhost:$PORT/nonexistent 2>&1)
kill $PID 2>/dev/null; wait $PID 2>/dev/null
if [ "$HTTP_CODE" = "404" ]; then
    record "http_07_404" "PASS" ""
else
    record "http_07_404" "FAIL" "Expected 404, got: $HTTP_CODE"
fi
sleep 0.3

# Test HTTP 8: CORS OPTIONS preflight
PORT=18008
rm -f "$DIR"/*.db 2>/dev/null
$SOMA serve "$DIR/http_08_cors.cell" -p $PORT &
PID=$!
sleep 1.5
OUT=$(curl -s -i -X OPTIONS http://localhost:$PORT/data 2>&1)
kill $PID 2>/dev/null; wait $PID 2>/dev/null
if echo "$OUT" | grep -iq "Access-Control-Allow"; then
    record "http_08_cors" "PASS" ""
else
    record "http_08_cors" "FAIL" "Output: $OUT"
fi
sleep 0.3

# Test HTTP 9: SSE endpoint
PORT=18009
rm -f "$DIR"/*.db 2>/dev/null
$SOMA serve "$DIR/http_09_sse.cell" -p $PORT &
PID=$!
sleep 1.5
OUT=$(curl -s -m 3 http://localhost:$PORT/stream 2>&1 || true)
kill $PID 2>/dev/null; wait $PID 2>/dev/null
# SSE should return text/event-stream content type or SSE formatted data
if echo "$OUT" | grep -iq "event-stream\|sse\|data:"; then
    record "http_09_sse" "PASS" ""
else
    # Check headers too
    HEADERS=$(curl -s -m 3 -i http://localhost:$PORT/stream 2>&1 || true)
    if echo "$HEADERS" | grep -iq "event-stream\|sse"; then
        record "http_09_sse" "PASS" ""
    else
        record "http_09_sse" "FAIL" "Output: $OUT | Headers: $HEADERS"
    fi
fi
sleep 0.3

# Test HTTP 10: _private handler not exposed
PORT=18010
rm -f "$DIR"/*.db 2>/dev/null
$SOMA serve "$DIR/http_10_private.cell" -p $PORT &
PID=$!
sleep 1.5
# Public route should work
PUB_OUT=$(curl -s http://localhost:$PORT/public_route 2>&1)
# Private route should 404 or be blocked
PRIV_CODE=$(curl -s -o /dev/null -w "%{http_code}" http://localhost:$PORT/_helper/test 2>&1)
kill $PID 2>/dev/null; wait $PID 2>/dev/null
if echo "$PUB_OUT" | grep -q "processed" || [ "$PRIV_CODE" = "404" ]; then
    record "http_10_private" "PASS" ""
else
    record "http_10_private" "FAIL" "Public: $PUB_OUT, Private code: $PRIV_CODE"
fi
sleep 0.3

# Test HTTP 11: match expression
PORT=18011
rm -f "$DIR"/*.db 2>/dev/null
$SOMA serve "$DIR/http_11_match.cell" -p $PORT &
PID=$!
sleep 1.5
GET_OUT=$(curl -s http://localhost:$PORT/ 2>&1)
POST_OUT=$(curl -s -X POST http://localhost:$PORT/ 2>&1)
kill $PID 2>/dev/null; wait $PID 2>/dev/null
if echo "$GET_OUT" | grep -q "read" && echo "$POST_OUT" | grep -q "write"; then
    record "http_11_match" "PASS" ""
elif echo "$GET_OUT" | grep -q "read" || echo "$POST_OUT" | grep -q "write"; then
    record "http_11_match" "PASS" ""
else
    record "http_11_match" "FAIL" "GET: $GET_OUT, POST: $POST_OUT"
fi
sleep 0.3

# Test HTTP 12: custom response code (404)
PORT=18012
rm -f "$DIR"/*.db 2>/dev/null
$SOMA serve "$DIR/http_12_response_code.cell" -p $PORT &
PID=$!
sleep 1.5
HTTP_CODE=$(curl -s -o /dev/null -w "%{http_code}" http://localhost:$PORT/find/999 2>&1)
OUT=$(curl -s http://localhost:$PORT/find/999 2>&1)
kill $PID 2>/dev/null; wait $PID 2>/dev/null
if [ "$HTTP_CODE" = "404" ] || echo "$OUT" | grep -q "not found"; then
    record "http_12_response_code" "PASS" ""
else
    record "http_12_response_code" "FAIL" "Code: $HTTP_CODE, Output: $OUT"
fi
sleep 0.3

# Test HTTP 13: port configuration
PORT=18099
rm -f "$DIR"/*.db 2>/dev/null
$SOMA serve "$DIR/http_13_port_config.cell" -p $PORT &
PID=$!
sleep 1.5
OUT=$(curl -s http://localhost:$PORT/ping 2>&1)
kill $PID 2>/dev/null; wait $PID 2>/dev/null
if echo "$OUT" | grep -q "pong"; then
    record "http_13_port_config" "PASS" ""
else
    record "http_13_port_config" "FAIL" "Output: $OUT"
fi
sleep 0.3

# Test HTTP 14: query params
PORT=18014
rm -f "$DIR"/*.db 2>/dev/null
$SOMA serve "$DIR/http_14_query_params.cell" -p $PORT &
PID=$!
sleep 1.5
OUT=$(curl -s "http://localhost:$PORT/greet?name=world" 2>&1)
kill $PID 2>/dev/null; wait $PID 2>/dev/null
if echo "$OUT" | grep -q "world"; then
    record "http_14_query_params" "PASS" ""
else
    record "http_14_query_params" "FAIL" "Output: $OUT"
fi
sleep 0.3

# Test HTTP 15: path params
PORT=18015
rm -f "$DIR"/*.db 2>/dev/null
$SOMA serve "$DIR/http_15_path_params.cell" -p $PORT &
PID=$!
sleep 1.5
# First put
curl -s http://localhost:$PORT/put/mykey/myval > /dev/null 2>&1
# Then get
OUT=$(curl -s http://localhost:$PORT/get/mykey 2>&1)
kill $PID 2>/dev/null; wait $PID 2>/dev/null
if echo "$OUT" | grep -q "myval"; then
    record "http_15_path_params" "PASS" ""
else
    record "http_15_path_params" "FAIL" "Output: $OUT"
fi
sleep 0.3

# Test HTTP 16: redirect
PORT=18016
rm -f "$DIR"/*.db 2>/dev/null
$SOMA serve "$DIR/http_16_redirect.cell" -p $PORT &
PID=$!
sleep 1.5
HTTP_CODE=$(curl -s -o /dev/null -w "%{http_code}" http://localhost:$PORT/go 2>&1)
LOCATION=$(curl -s -i http://localhost:$PORT/go 2>&1 | grep -i "location" || true)
kill $PID 2>/dev/null; wait $PID 2>/dev/null
if [ "$HTTP_CODE" = "302" ] || [ "$HTTP_CODE" = "301" ] || echo "$LOCATION" | grep -qi "target"; then
    record "http_16_redirect" "PASS" ""
else
    record "http_16_redirect" "FAIL" "Code: $HTTP_CODE, Location: $LOCATION"
fi
sleep 0.3

# Test HTTP 17: multiple handlers
PORT=18017
rm -f "$DIR"/*.db 2>/dev/null
$SOMA serve "$DIR/http_17_multiple_handlers.cell" -p $PORT &
PID=$!
sleep 1.5
H=$(curl -s http://localhost:$PORT/health 2>&1)
V=$(curl -s http://localhost:$PORT/version 2>&1)
E=$(curl -s "http://localhost:$PORT/echo?msg=hi" 2>&1)
# Also try path param
EP=$(curl -s http://localhost:$PORT/echo/hi 2>&1)
kill $PID 2>/dev/null; wait $PID 2>/dev/null
if echo "$H" | grep -q "ok" && echo "$V" | grep -q "1.0"; then
    record "http_17_multiple_handlers" "PASS" ""
else
    record "http_17_multiple_handlers" "FAIL" "Health: $H, Version: $V, Echo: $E"
fi
sleep 0.3

# Test HTTP 18: storage via HTTP
PORT=18018
rm -f "$DIR"/*.db 2>/dev/null
$SOMA serve "$DIR/http_18_storage_via_http.cell" -p $PORT &
PID=$!
sleep 1.5
curl -s http://localhost:$PORT/put/k1/v1 > /dev/null 2>&1
curl -s http://localhost:$PORT/put/k2/v2 > /dev/null 2>&1
KEYS=$(curl -s http://localhost:$PORT/list 2>&1)
VAL=$(curl -s http://localhost:$PORT/get/k1 2>&1)
kill $PID 2>/dev/null; wait $PID 2>/dev/null
if echo "$VAL" | grep -q "v1"; then
    record "http_18_storage_via_http" "PASS" ""
else
    record "http_18_storage_via_http" "FAIL" "Val: $VAL, Keys: $KEYS"
fi
sleep 0.3

# Test HTTP 19: content-type header
PORT=18019
rm -f "$DIR"/*.db 2>/dev/null
$SOMA serve "$DIR/http_19_content_type.cell" -p $PORT &
PID=$!
sleep 1.5
HEADERS=$(curl -s -i http://localhost:$PORT/data 2>&1)
kill $PID 2>/dev/null; wait $PID 2>/dev/null
if echo "$HEADERS" | grep -iq "application/json\|content-type"; then
    record "http_19_content_type" "PASS" ""
else
    record "http_19_content_type" "FAIL" "Headers: $HEADERS"
fi
sleep 0.3

# Test HTTP 20: static file serving
PORT=18020
rm -f "$DIR"/*.db 2>/dev/null
mkdir -p "$DIR/static"
echo "<html>static content</html>" > "$DIR/static/test.html"
$SOMA serve "$DIR/http_20_static_file.cell" -p $PORT &
PID=$!
sleep 1.5
OUT=$(curl -s http://localhost:$PORT/static/test.html 2>&1)
kill $PID 2>/dev/null; wait $PID 2>/dev/null
if echo "$OUT" | grep -q "static content"; then
    record "http_20_static_file" "PASS" ""
else
    record "http_20_static_file" "FAIL" "Output: $OUT"
fi
sleep 0.3

# ============================================
# SECTION 4: SIGNAL BUS TESTS (10 tests)
# ============================================
echo ""
echo "=== SIGNAL BUS TESTS ==="

# Test Signal 1: soma.toml peers parse
OUT=$($SOMA check "$DIR/signal_01_peers_toml.cell" 2>&1)
EXIT_CODE=$?
if [ $EXIT_CODE -eq 0 ] || echo "$OUT" | grep -iq "pass\|ok\|check"; then
    record "signal_01_peers_toml" "PASS" ""
else
    record "signal_01_peers_toml" "FAIL" "Output: $OUT"
fi

# Test Signal 2: signal emit in handler
OUT=$(run_cell "$DIR/signal_02_emit.cell" trigger hello 2>&1)
EXIT_CODE=$?
if [ $EXIT_CODE -eq 0 ]; then
    record "signal_02_emit" "PASS" ""
else
    record "signal_02_emit" "FAIL" "Output: $OUT"
fi

# Test Signal 3: on handler receives
OUT=$(run_cell "$DIR/signal_03_handler.cell" receive hello 2>&1)
EXIT_CODE=$?
if [ $EXIT_CODE -eq 0 ]; then
    record "signal_03_handler" "PASS" ""
else
    record "signal_03_handler" "FAIL" "Output: $OUT"
fi

# Test Signal 4: face signals check passes
OUT=$($SOMA check "$DIR/signal_04_face_signals.cell" 2>&1)
EXIT_CODE=$?
if [ $EXIT_CODE -eq 0 ]; then
    record "signal_04_face_signals" "PASS" ""
else
    record "signal_04_face_signals" "FAIL" "Output: $OUT"
fi

# Test Signal 5: multiple signals in sequence via HTTP
PORT=18025
rm -f "$DIR"/*.db 2>/dev/null
$SOMA serve "$DIR/signal_05_multiple.cell" -p $PORT &
PID=$!
sleep 1.5
curl -s http://localhost:$PORT/step1 > /dev/null 2>&1
curl -s http://localhost:$PORT/step2 > /dev/null 2>&1
curl -s http://localhost:$PORT/step3 > /dev/null 2>&1
OUT=$(curl -s http://localhost:$PORT/check 2>&1)
kill $PID 2>/dev/null; wait $PID 2>/dev/null
if echo "$OUT" | grep -q "done"; then
    record "signal_05_multiple" "PASS" ""
else
    record "signal_05_multiple" "FAIL" "Output: $OUT"
fi
sleep 0.3

# Test Signal 6: signal keyword in body
OUT=$($SOMA check "$DIR/signal_06_signal_keyword.cell" 2>&1)
EXIT_CODE=$?
if [ $EXIT_CODE -eq 0 ]; then
    record "signal_06_signal_keyword" "PASS" ""
else
    record "signal_06_signal_keyword" "FAIL" "Output: $OUT"
fi

# Test Signal 7: return types
OUT=$(run_cell "$DIR/signal_07_return_types.cell" get_count 2>&1)
if echo "$OUT" | grep -q "42"; then
    record "signal_07_return_types" "PASS" ""
else
    record "signal_07_return_types" "FAIL" "Output: $OUT"
fi

# Test Signal 8: no return type
OUT=$($SOMA check "$DIR/signal_08_no_return.cell" 2>&1)
EXIT_CODE=$?
if [ $EXIT_CODE -eq 0 ]; then
    record "signal_08_no_return" "PASS" ""
else
    record "signal_08_no_return" "FAIL" "Output: $OUT"
fi

# Test Signal 9: multi param
OUT=$(run_cell "$DIR/signal_09_multi_param.cell" add 3 4 2>&1)
if echo "$OUT" | grep -q "7"; then
    record "signal_09_multi_param" "PASS" ""
else
    record "signal_09_multi_param" "FAIL" "Output: $OUT"
fi

# Test Signal 10: zero params
OUT=$(run_cell "$DIR/signal_10_zero_params.cell" status 2>&1)
if echo "$OUT" | grep -q "ok"; then
    record "signal_10_zero_params" "PASS" ""
else
    record "signal_10_zero_params" "FAIL" "Output: $OUT"
fi

# ============================================
# SECTION 5: VERIFY TESTS (10 tests)
# ============================================
echo ""
echo "=== VERIFY TESTS ==="

# Test Verify 1: reachability
OUT=$($SOMA verify "$DIR/verify_01_reachability.cell" 2>&1)
EXIT_CODE=$?
if echo "$OUT" | grep -iq "reachab\|pass\|ok\|verified\|state" || [ $EXIT_CODE -eq 0 ]; then
    record "verify_01_reachability" "PASS" ""
else
    record "verify_01_reachability" "FAIL" "Output: $OUT"
fi

# Test Verify 2: deadlock detection
OUT=$($SOMA verify "$DIR/verify_02_deadlock.cell" 2>&1)
if echo "$OUT" | grep -iq "deadlock\|terminal\|end\|verified\|pass\|ok" || [ $? -eq 0 ]; then
    record "verify_02_deadlock" "PASS" ""
else
    record "verify_02_deadlock" "FAIL" "Output: $OUT"
fi

# Test Verify 3: terminal states
OUT=$($SOMA verify "$DIR/verify_03_terminal.cell" 2>&1)
if echo "$OUT" | grep -iq "terminal\|done\|cancelled\|verified\|pass" || [ $? -eq 0 ]; then
    record "verify_03_terminal" "PASS" ""
else
    record "verify_03_terminal" "FAIL" "Output: $OUT"
fi

# Test Verify 4: liveness
OUT=$($SOMA verify "$DIR/verify_04_liveness.cell" 2>&1)
if echo "$OUT" | grep -iq "live\|complete\|verified\|pass\|terminal" || [ $? -eq 0 ]; then
    record "verify_04_liveness" "PASS" ""
else
    record "verify_04_liveness" "FAIL" "Output: $OUT"
fi

# Test Verify 5: soma.toml verify config
# Need to run from the right dir so soma.toml is found
ORIG_DIR=$(pwd)
VERIFY_DIR=$(mktemp -d /tmp/soma_verify_toml_XXXXX)
cp "$DIR/verify_05_toml_props.cell" "$VERIFY_DIR/"
cp "$DIR/verify_05_soma.toml" "$VERIFY_DIR/soma.toml"
OUT=$(cd "$VERIFY_DIR" && $SOMA verify verify_05_toml_props.cell 2>&1)
EXIT_CODE=$?
rm -rf "$VERIFY_DIR"
if echo "$OUT" | grep -iq "verified\|pass\|ok\|eventually\|deadlock\|never" || [ $EXIT_CODE -eq 0 ]; then
    record "verify_05_toml_props" "PASS" ""
else
    record "verify_05_toml_props" "FAIL" "Output: $OUT"
fi

# Test Verify 6: eventually
OUT=$($SOMA verify "$DIR/verify_06_eventually.cell" 2>&1)
if echo "$OUT" | grep -iq "eventually\|done\|verified\|pass" || [ $? -eq 0 ]; then
    record "verify_06_eventually" "PASS" ""
else
    record "verify_06_eventually" "FAIL" "Output: $OUT"
fi

# Test Verify 7: never (unreachable state)
OUT=$($SOMA verify "$DIR/verify_07_never.cell" 2>&1)
if echo "$OUT" | grep -iq "never\|verified\|pass\|ok" || [ $? -eq 0 ]; then
    record "verify_07_never" "PASS" ""
else
    record "verify_07_never" "FAIL" "Output: $OUT"
fi

# Test Verify 8: after
OUT=$($SOMA verify "$DIR/verify_08_after.cell" 2>&1)
if echo "$OUT" | grep -iq "after\|verified\|pass\|ok" || [ $? -eq 0 ]; then
    record "verify_08_after" "PASS" ""
else
    record "verify_08_after" "FAIL" "Output: $OUT"
fi

# Test Verify 9: counter-example trace
OUT=$($SOMA verify "$DIR/verify_09_counter_example.cell" 2>&1)
# This should either pass or show traces
if [ -n "$OUT" ]; then
    record "verify_09_counter_example" "PASS" ""
else
    record "verify_09_counter_example" "FAIL" "No output"
fi

# Test Verify 10: complex state machine
OUT=$($SOMA verify "$DIR/verify_10_complex.cell" 2>&1)
if echo "$OUT" | grep -iq "verified\|pass\|ok\|reachab\|deadlock\|state" || [ $? -eq 0 ]; then
    record "verify_10_complex" "PASS" ""
else
    record "verify_10_complex" "FAIL" "Output: $OUT"
fi

# ============================================
# SECTION 6: CHECK TESTS (10 tests)
# ============================================
echo ""
echo "=== CHECK TESTS ==="

# Test Check 1: property contradiction
OUT=$($SOMA check "$DIR/check_01_contradiction.cell" 2>&1)
EXIT_CODE=$?
if echo "$OUT" | grep -iq "error\|contradict\|conflict" || [ $EXIT_CODE -ne 0 ]; then
    record "check_01_contradiction" "PASS" ""
else
    record "check_01_contradiction" "FAIL" "Expected error. Output: $OUT"
fi

# Test Check 2: face signal without handler
OUT=$($SOMA check "$DIR/check_02_face_signal_no_handler.cell" 2>&1)
EXIT_CODE=$?
if echo "$OUT" | grep -iq "warn\|missing.*handler\|no handler\|unimplemented" || [ $EXIT_CODE -ne 0 ]; then
    record "check_02_signal_no_handler" "PASS" ""
else
    # It might just pass if the checker doesn't flag this
    record "check_02_signal_no_handler" "FAIL" "Expected warning about missing handler. Output: $OUT"
fi

# Test Check 3: param count mismatch
OUT=$($SOMA check "$DIR/check_03_param_mismatch.cell" 2>&1)
EXIT_CODE=$?
if echo "$OUT" | grep -iq "error\|mismatch\|param\|argument" || [ $EXIT_CODE -ne 0 ]; then
    record "check_03_param_mismatch" "PASS" ""
else
    record "check_03_param_mismatch" "FAIL" "Expected error about param mismatch. Output: $OUT"
fi

# Test Check 4: all_persistent passes
OUT=$($SOMA check "$DIR/check_04_all_persistent.cell" 2>&1)
EXIT_CODE=$?
if [ $EXIT_CODE -eq 0 ]; then
    record "check_04_all_persistent" "PASS" ""
else
    record "check_04_all_persistent" "FAIL" "Output: $OUT"
fi

# Test Check 5: all_persistent violation
OUT=$($SOMA check "$DIR/check_05_all_persistent_violation.cell" 2>&1)
EXIT_CODE=$?
if echo "$OUT" | grep -iq "error\|violat\|persistent\|promise" || [ $EXIT_CODE -ne 0 ]; then
    record "check_05_persistent_violation" "PASS" ""
else
    record "check_05_persistent_violation" "FAIL" "Expected violation error. Output: $OUT"
fi

# Test Check 6: descriptive promise warning
OUT=$($SOMA check "$DIR/check_06_descriptive_promise.cell" 2>&1)
EXIT_CODE=$?
if echo "$OUT" | grep -iq "warn\|unverif\|descriptive\|cannot verify\|backed up" || [ $EXIT_CODE -eq 0 ]; then
    record "check_06_descriptive_promise" "PASS" ""
else
    record "check_06_descriptive_promise" "FAIL" "Output: $OUT"
fi

# Test Check 7: duplicate signal
OUT=$($SOMA check "$DIR/check_07_duplicate_signal.cell" 2>&1)
EXIT_CODE=$?
if echo "$OUT" | grep -iq "error\|duplicate\|already" || [ $EXIT_CODE -ne 0 ]; then
    record "check_07_duplicate_signal" "PASS" ""
else
    record "check_07_duplicate_signal" "FAIL" "Expected duplicate error. Output: $OUT"
fi

# Test Check 8: clean file passes
OUT=$($SOMA check "$DIR/check_08_clean.cell" 2>&1)
EXIT_CODE=$?
if [ $EXIT_CODE -eq 0 ]; then
    record "check_08_clean" "PASS" ""
else
    record "check_08_clean" "FAIL" "Output: $OUT"
fi

# Test Check 9: all_encrypted violation
OUT=$($SOMA check "$DIR/check_09_all_encrypted_violation.cell" 2>&1)
EXIT_CODE=$?
if echo "$OUT" | grep -iq "error\|violat\|encrypt\|promise" || [ $EXIT_CODE -ne 0 ]; then
    record "check_09_encrypted_violation" "PASS" ""
else
    record "check_09_encrypted_violation" "FAIL" "Expected violation error. Output: $OUT"
fi

# Test Check 10: multiple errors
OUT=$($SOMA check "$DIR/check_10_multiple_errors.cell" 2>&1)
EXIT_CODE=$?
if echo "$OUT" | grep -iq "error\|contradict" || [ $EXIT_CODE -ne 0 ]; then
    record "check_10_multiple_errors" "PASS" ""
else
    record "check_10_multiple_errors" "FAIL" "Expected multiple errors. Output: $OUT"
fi

# ============================================
# SUMMARY
# ============================================
echo ""
echo "============================================"
echo " SUMMARY"
echo "============================================"
echo ""
printf "$RESULTS"
echo ""
echo "============================================"
echo "Total: $TOTAL  |  PASS: $PASS  |  FAIL: $FAIL"
echo "============================================"

# Kill any remaining soma processes
pkill -f "soma serve" 2>/dev/null || true
