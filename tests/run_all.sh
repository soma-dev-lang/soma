#!/bin/bash
# Exhaustive Soma v1 test runner
SOMA="/Users/antoine/paradigm/compiler/target/release/soma"
TEST_DIR="/tmp/soma_v1_tests/core"
PASS=0
FAIL=0
FAILURES=""

run_test() {
    local name="$1"
    local file="$2"
    local signal="$3"
    local expected="$4"
    shift 4
    local args=("$@")

    local actual
    actual=$(perl -e 'alarm 10; exec @ARGV' "$SOMA" run "$file" "$signal" "${args[@]}" 2>&1)
    local exit_code=$?

    if [ "$expected" = "__ERROR__" ]; then
        if [ $exit_code -ne 0 ] || echo "$actual" | grep -qi "error\|panic\|overflow"; then
            echo "PASS $name: got error as expected"
            PASS=$((PASS + 1))
            return
        else
            echo "FAIL $name: expected error, got: $(echo "$actual" | head -1)"
            FAIL=$((FAIL + 1))
            FAILURES="$FAILURES\n  $name: expected error, got: $(echo "$actual" | head -1)"
            return
        fi
    fi

    # Normalize: trim trailing whitespace/newlines
    local norm_expected norm_actual
    norm_expected=$(printf '%s' "$expected" | sed 's/[[:space:]]*$//')
    norm_actual=$(printf '%s' "$actual" | sed 's/[[:space:]]*$//')

    if [ "$norm_actual" = "$norm_expected" ]; then
        echo "PASS $name: $(echo "$expected" | head -1)"
        PASS=$((PASS + 1))
    else
        echo "FAIL $name"
        echo "  expected: |$(echo "$norm_expected" | head -3)|"
        echo "  actual:   |$(echo "$norm_actual" | head -3)|"
        FAIL=$((FAIL + 1))
        FAILURES="$FAILURES\n  $name: expected [$(echo "$norm_expected" | head -1)], got [$(echo "$norm_actual" | head -1)]"
    fi
}

# Helper to build expected multiline strings
E() { printf '%s\n' "$@" | sed '$ s/\n$//'; }

echo "========================================"
echo "  SOMA v1 EXHAUSTIVE CORE TESTS"
echo "========================================"
echo ""

# =========================================================================
# 1. LITERALS (18 tests)
# =========================================================================
echo "--- 1. LITERALS ---"

run_test "core/literals/int_zero" "$TEST_DIR/literals/int_zero.cell" run "$(E '0' 'done')"
run_test "core/literals/int_one" "$TEST_DIR/literals/int_one.cell" run "$(E '1' 'done')"
run_test "core/literals/int_neg_one" "$TEST_DIR/literals/int_neg_one.cell" run "$(E '-1' 'done')"
run_test "core/literals/int_42" "$TEST_DIR/literals/int_42.cell" run "$(E '42' 'done')"
run_test "core/literals/int_i64_min" "$TEST_DIR/literals/int_i64_min.cell" run "$(E '-9223372036854775808' 'done')"
run_test "core/literals/int_i64_max" "$TEST_DIR/literals/int_i64_max.cell" run "$(E '9223372036854775807' 'done')"
run_test "core/literals/float_zero" "$TEST_DIR/literals/float_zero.cell" run "$(E '0.0' 'done')"
run_test "core/literals/float_pi" "$TEST_DIR/literals/float_pi.cell" run "$(E '3.14' 'done')"
run_test "core/literals/float_neg" "$TEST_DIR/literals/float_neg.cell" run "$(E '-2.5' 'done')"
run_test "core/literals/float_sci_pos" "$TEST_DIR/literals/float_sci_pos.cell" run "$(E '1500.0' 'done')"
run_test "core/literals/float_sci_neg" "$TEST_DIR/literals/float_sci_neg.cell" run "$(E '0.0025' 'done')"
run_test "core/literals/string_empty" "$TEST_DIR/literals/string_empty.cell" run "$(E '' 'done')"
run_test "core/literals/string_hello" "$TEST_DIR/literals/string_hello.cell" run "$(E 'hello' 'done')"
run_test "core/literals/bool_true" "$TEST_DIR/literals/bool_true.cell" run "$(E 'true' 'done')"
run_test "core/literals/bool_false" "$TEST_DIR/literals/bool_false.cell" run "$(E 'false' 'done')"
run_test "core/literals/unit_value" "$TEST_DIR/literals/unit_value.cell" run "$(E 'null' 'done')"
run_test "core/literals/string_interpolation" "$TEST_DIR/literals/string_interpolation.cell" run "$(E 'hello world' 'done')"
run_test "core/literals/string_triple_quote" "$TEST_DIR/literals/string_triple_quote.cell" run "done"

echo ""

# =========================================================================
# 2. VARIABLES (10 tests)
# =========================================================================
echo "--- 2. VARIABLES ---"

run_test "core/variables/let_binding" "$TEST_DIR/variables/let_binding.cell" run "$(E '42' 'done')"
run_test "core/variables/reassignment" "$TEST_DIR/variables/reassignment.cell" run "$(E '10' '20' 'done')"
run_test "core/variables/plus_equals" "$TEST_DIR/variables/plus_equals.cell" run "$(E '15' 'done')"
run_test "core/variables/shadowing" "$TEST_DIR/variables/shadowing.cell" run "$(E 'hello' '42' 'done')"
run_test "core/variables/block_scope_if" "$TEST_DIR/variables/block_scope_if.cell" run "$(E '99' 'done')"
run_test "core/variables/outer_propagation" "$TEST_DIR/variables/outer_propagation.cell" run "$(E '100' 'done')"
run_test "core/variables/for_loop_scope" "$TEST_DIR/variables/for_loop_scope.cell" run "$(E '6' 'done')"
run_test "core/variables/string_plus_equals" "$TEST_DIR/variables/string_plus_equals.cell" run "$(E 'hello world' 'done')"
run_test "core/variables/multiple_let" "$TEST_DIR/variables/multiple_let.cell" run "$(E '1' '2' '3' 'done')"
run_test "core/variables/reassign_type_change" "$TEST_DIR/variables/reassign_type_change.cell" run "$(E '42' 'hello' 'done')"

echo ""

# =========================================================================
# 3. OPERATORS (20 tests)
# =========================================================================
echo "--- 3. OPERATORS ---"

run_test "core/operators/add_int" "$TEST_DIR/operators/add_int.cell" run "$(E '5' 'done')"
run_test "core/operators/sub_int" "$TEST_DIR/operators/sub_int.cell" run "$(E '3' 'done')"
run_test "core/operators/mul_int" "$TEST_DIR/operators/mul_int.cell" run "$(E '12' 'done')"
run_test "core/operators/div_int" "$TEST_DIR/operators/div_int.cell" run "$(E '3' 'done')"
run_test "core/operators/mod_int" "$TEST_DIR/operators/mod_int.cell" run "$(E '1' 'done')"
run_test "core/operators/add_float" "$TEST_DIR/operators/add_float.cell" run "$(E '3.5' 'done')"
run_test "core/operators/mul_float" "$TEST_DIR/operators/mul_float.cell" run "$(E '6.28' 'done')"
run_test "core/operators/eq_int" "$TEST_DIR/operators/eq_int.cell" run "$(E 'true' 'false' 'done')"
run_test "core/operators/neq_int" "$TEST_DIR/operators/neq_int.cell" run "$(E 'true' 'false' 'done')"
run_test "core/operators/lt_gt" "$TEST_DIR/operators/lt_gt.cell" run "$(E 'true' 'true' 'true' 'true' 'done')"
run_test "core/operators/eq_string" "$TEST_DIR/operators/eq_string.cell" run "$(E 'true' 'false' 'done')"
run_test "core/operators/and_logic" "$TEST_DIR/operators/and_logic.cell" run "$(E 'true' 'false' 'done')"
run_test "core/operators/or_logic" "$TEST_DIR/operators/or_logic.cell" run "$(E 'true' 'false' 'done')"
run_test "core/operators/not_logic" "$TEST_DIR/operators/not_logic.cell" run "$(E 'false' 'true' 'done')"
run_test "core/operators/and_short_circuit" "$TEST_DIR/operators/and_short_circuit.cell" run "$(E 'false' 'no side effect' 'done')"
run_test "core/operators/or_short_circuit" "$TEST_DIR/operators/or_short_circuit.cell" run "$(E 'true' 'no side effect' 'done')"
run_test "core/operators/null_coalesce" "$TEST_DIR/operators/null_coalesce.cell" run "$(E '42' '10' 'done')"
run_test "core/operators/pipe" "$TEST_DIR/operators/pipe.cell" run "$(E 'HELLO' 'done')"
run_test "core/operators/string_concat" "$TEST_DIR/operators/string_concat.cell" run "$(E 'hello world' 'done')"
run_test "core/operators/comparison_float" "$TEST_DIR/operators/comparison_float.cell" run "$(E 'true' 'true' 'done')"

echo ""

# =========================================================================
# 4. CONTROL FLOW (15 tests)
# =========================================================================
echo "--- 4. CONTROL FLOW ---"

run_test "core/control/if_true" "$TEST_DIR/control_flow/if_true.cell" run "$(E 'yes' 'done')"
run_test "core/control/if_false" "$TEST_DIR/control_flow/if_false.cell" run "$(E 'no' 'done')"
run_test "core/control/else_if" "$TEST_DIR/control_flow/else_if.cell" run "$(E 'medium' 'done')"
run_test "core/control/nested_if" "$TEST_DIR/control_flow/nested_if.cell" run "$(E 'both' 'done')"
run_test "core/control/while_basic" "$TEST_DIR/control_flow/while_basic.cell" run "$(E '0' '1' '2' 'done')"
run_test "core/control/while_break" "$TEST_DIR/control_flow/while_break.cell" run "$(E '5' 'done')"
run_test "core/control/for_in_list" "$TEST_DIR/control_flow/for_in_list.cell" run "$(E 'a' 'b' 'c' 'done')"
run_test "core/control/for_in_range" "$TEST_DIR/control_flow/for_in_range.cell" run "$(E '1' '2' '3' '4' 'done')"
run_test "core/control/continue_loop" "$TEST_DIR/control_flow/continue_loop.cell" run "$(E '1' '3' '5' 'done')"
run_test "core/control/match_int" "$TEST_DIR/control_flow/match_int.cell" run "$(E 'two' 'done')"
run_test "core/control/match_string" "$TEST_DIR/control_flow/match_string.cell" run "$(E 'greeting' 'done')"
run_test "core/control/match_wildcard" "$TEST_DIR/control_flow/match_wildcard.cell" run "$(E 'other' 'done')"
run_test "core/control/match_expr" "$TEST_DIR/control_flow/match_expr.cell" run "$(E 'two' 'done')"
run_test "core/control/match_block" "$TEST_DIR/control_flow/match_block.cell" run "$(E 'handling two' 'two' 'done')"
run_test "core/control/match_unit" "$TEST_DIR/control_flow/match_unit.cell" run "$(E 'was null' 'done')"

echo ""

# =========================================================================
# 5. FUNCTIONS (10 tests)
# =========================================================================
echo "--- 5. FUNCTIONS ---"

run_test "core/functions/handler_call" "$TEST_DIR/functions/handler_call.cell" describe "$(E 'Rectangle 5 x 3' 'Area: 15' 'Perimeter: 16' 'done')" 5 3
run_test "core/functions/factorial" "$TEST_DIR/functions/factorial.cell" factorial "720" 6
run_test "core/functions/fibonacci" "$TEST_DIR/functions/fibonacci.cell" fibonacci "55" 10
run_test "core/functions/return_value" "$TEST_DIR/functions/return_value.cell" run "$(E '42' 'done')"
run_test "core/functions/handler_chain" "$TEST_DIR/functions/handler_chain.cell" run "$(E '30' 'done')"
run_test "core/functions/arity_too_few" "$TEST_DIR/functions/arity_too_few.cell" add "__ERROR__" 5
run_test "core/functions/arity_too_many" "$TEST_DIR/functions/arity_too_many.cell" add "__ERROR__" 1 2 3
run_test "core/functions/recursion_deep" "$TEST_DIR/functions/recursion_deep.cell" sum_to "5050" 100
run_test "core/functions/early_return" "$TEST_DIR/functions/early_return.cell" run "$(E 'found' 'done')"
run_test "core/functions/private_handler" "$TEST_DIR/functions/private_handler.cell" run "$(E '42' 'done')"

echo ""

# =========================================================================
# 6. LAMBDAS (10 tests)
# =========================================================================
echo "--- 6. LAMBDAS ---"

run_test "core/lambdas/map_lambda" "$TEST_DIR/lambdas/map_lambda.cell" run "$(E '[2, 4, 6]' 'done')"
run_test "core/lambdas/filter_lambda" "$TEST_DIR/lambdas/filter_lambda.cell" run "$(E '[2, 4]' 'done')"
run_test "core/lambdas/find_lambda" "$TEST_DIR/lambdas/find_lambda.cell" run "$(E '3' 'done')"
run_test "core/lambdas/any_lambda" "$TEST_DIR/lambdas/any_lambda.cell" run "$(E 'true' 'false' 'done')"
run_test "core/lambdas/all_lambda" "$TEST_DIR/lambdas/all_lambda.cell" run "$(E 'true' 'false' 'done')"
run_test "core/lambdas/count_lambda" "$TEST_DIR/lambdas/count_lambda.cell" run "$(E '3' 'done')"
run_test "core/lambdas/block_lambda" "$TEST_DIR/lambdas/block_lambda.cell" run "$(E '[1, 4, 9]' 'done')"
run_test "core/lambdas/closure" "$TEST_DIR/lambdas/closure.cell" run "$(E '[11, 12, 13]' 'done')"
run_test "core/lambdas/lambda_chain" "$TEST_DIR/lambdas/lambda_chain.cell" run "$(E '[4, 6, 8, 10]' 'done')"
run_test "core/lambdas/reduce_lambda" "$TEST_DIR/lambdas/reduce_lambda.cell" run "$(E '15' 'done')"

echo ""

# =========================================================================
# 7. COLLECTIONS (15 tests)
# =========================================================================
echo "--- 7. COLLECTIONS ---"

run_test "core/collections/list_create" "$TEST_DIR/collections/list_create.cell" run "$(E '[1, 2, 3]' 'done')"
run_test "core/collections/list_push" "$TEST_DIR/collections/list_push.cell" run "$(E '[1, 2, 3, 4]' 'done')"
run_test "core/collections/list_len" "$TEST_DIR/collections/list_len.cell" run "$(E '3' 'done')"
run_test "core/collections/list_reverse" "$TEST_DIR/collections/list_reverse.cell" run "$(E '[3, 2, 1]' 'done')"
run_test "core/collections/list_range" "$TEST_DIR/collections/list_range.cell" run "$(E '[0, 1, 2, 3, 4]' 'done')"
run_test "core/collections/map_create" "$TEST_DIR/collections/map_create.cell" run "$(E 'Alice' '30' 'done')"
run_test "core/collections/map_with" "$TEST_DIR/collections/map_with.cell" run "$(E 'alice@ex.com' 'done')"
run_test "core/collections/map_keys_values" "$TEST_DIR/collections/map_keys_values.cell" run "$(E 'ok' 'done')"
run_test "core/collections/list_concat" "$TEST_DIR/collections/list_concat.cell" run "$(E '[1, 2, 3, 4]' 'done')"
run_test "core/collections/for_in_map" "$TEST_DIR/collections/for_in_map.cell" run "$(E 'ok' 'done')"
run_test "core/collections/nested_list_maps" "$TEST_DIR/collections/nested_list_maps.cell" run "$(E 'Alice' '30' 'done')"
run_test "core/collections/sort_by" "$TEST_DIR/collections/sort_by.cell" run "$(E 'Charlie' 'Bob' 'Alice' 'done')"
run_test "core/collections/filter_by" "$TEST_DIR/collections/filter_by.cell" run "$(E '2' 'done')"
run_test "core/collections/flatten_zip" "$TEST_DIR/collections/flatten_zip.cell" run "$(E '[1, 2, 3, 4]' 'a:1' 'done')"
run_test "core/collections/group_by" "$TEST_DIR/collections/group_by.cell" run "$(E 'ok' 'done')"

echo ""

# =========================================================================
# 8. STRINGS (10 tests)
# =========================================================================
echo "--- 8. STRINGS ---"

run_test "core/strings/interpolation_var" "$TEST_DIR/strings/interpolation_var.cell" run "$(E 'hello world' 'done')"
run_test "core/strings/interpolation_map" "$TEST_DIR/strings/interpolation_map.cell" run "$(E 'name: Alice' 'done')"
run_test "core/strings/len_str" "$TEST_DIR/strings/len_str.cell" run "$(E '5' 'done')"
run_test "core/strings/contains_str" "$TEST_DIR/strings/contains_str.cell" run "$(E 'true' 'false' 'done')"
run_test "core/strings/starts_ends" "$TEST_DIR/strings/starts_ends.cell" run "$(E 'true' 'true' 'done')"
run_test "core/strings/replace_str" "$TEST_DIR/strings/replace_str.cell" run "$(E 'hello world' 'done')"
run_test "core/strings/split_str" "$TEST_DIR/strings/split_str.cell" run "$(E '["a", "b", "c"]' 'done')"
run_test "core/strings/trim_str" "$TEST_DIR/strings/trim_str.cell" run "$(E 'hello' 'done')"
run_test "core/strings/upper_lower" "$TEST_DIR/strings/upper_lower.cell" run "$(E 'HELLO' 'hello' 'done')"
run_test "core/strings/substring_indexof" "$TEST_DIR/strings/substring_indexof.cell" run "$(E 'hel' '2' 'done')"

echo ""

# =========================================================================
# 9. ERROR HANDLING (10 tests)
# =========================================================================
echo "--- 9. ERROR HANDLING ---"

run_test "core/errors/try_good" "$TEST_DIR/error_handling/try_good.cell" run "$(E '42' 'null' 'done')"
run_test "core/errors/try_div_zero" "$TEST_DIR/error_handling/try_div_zero.cell" run "$(E 'null' 'division by zero' 'done')"
run_test "core/errors/try_undefined" "$TEST_DIR/error_handling/try_undefined.cell" run "$(E 'caught' 'done')"
run_test "core/errors/stack_overflow" "$TEST_DIR/error_handling/stack_overflow.cell" run "__ERROR__"
run_test "core/errors/to_int_bad" "$TEST_DIR/error_handling/to_int_bad.cell" run "$(E 'null' 'done')"
run_test "core/errors/to_float_bad" "$TEST_DIR/error_handling/to_float_bad.cell" run "$(E 'null' 'done')"
run_test "core/errors/try_nested" "$TEST_DIR/error_handling/try_nested.cell" run "$(E '42' 'caught' 'done')"
run_test "core/errors/try_value_field" "$TEST_DIR/error_handling/try_value_field.cell" run "$(E '10' 'done')"
run_test "core/errors/try_error_field" "$TEST_DIR/error_handling/try_error_field.cell" run "$(E 'division by zero' 'done')"
run_test "core/errors/try_recover" "$TEST_DIR/error_handling/try_recover.cell" run "$(E '0' 'done')"

echo ""

# =========================================================================
# SUMMARY
# =========================================================================
echo "========================================"
echo "  SUMMARY"
echo "========================================"
TOTAL=$((PASS + FAIL))
echo "  Total:  $TOTAL"
echo "  Passed: $PASS"
echo "  Failed: $FAIL"
if [ $FAIL -gt 0 ]; then
    echo ""
    echo "  FAILURES:"
    echo -e "$FAILURES"
fi
echo "========================================"
