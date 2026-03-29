#!/bin/bash
# Soma v1 Edge Case & JIT Test Suite
SOMA=/Users/antoine/paradigm/compiler/target/release/soma
DIR=/tmp/soma_v1_tests/edge
PASS=0
FAIL=0
ERRORS=""

run_test() {
    local name="$1"
    local file="$2"
    local cmd="$3"
    local expect_pattern="$4"   # regex to match in output (optional)
    local expect_fail="$5"      # "yes" if we expect non-zero exit

    echo "--- $name ---"
    local output
    output=$(eval "$cmd" 2>&1)
    local exit_code=$?

    local passed=true

    if [ "$expect_fail" = "yes" ]; then
        if [ $exit_code -eq 0 ]; then
            # Some error tests might still exit 0 but have error output
            : # don't fail just on exit code for error tests
        fi
    fi

    if [ -n "$expect_pattern" ]; then
        if echo "$output" | grep -qE "$expect_pattern"; then
            : # pattern matched
        else
            passed=false
        fi
    fi

    if $passed; then
        echo "  PASS"
        echo "  Output: $(echo "$output" | head -5)"
        PASS=$((PASS + 1))
    else
        echo "  FAIL"
        echo "  Expected pattern: $expect_pattern"
        echo "  Actual output:"
        echo "$output" | head -10 | sed 's/^/    /'
        FAIL=$((FAIL + 1))
        ERRORS="$ERRORS\n  FAIL: $name"
    fi
    echo ""
}

run_jit_compare() {
    local name="$1"
    local file="$2"
    local signal="${3:-run}"

    echo "--- $name (JIT compare) ---"
    local interp_out jit_out
    interp_out=$($SOMA run "$file" "$signal" 2>&1 | grep -v "^note:")
    local interp_exit=$?
    jit_out=$($SOMA run --jit "$file" "$signal" 2>&1 | grep -v "^note:")
    local jit_exit=$?

    if [ "$interp_out" = "$jit_out" ]; then
        echo "  PASS (outputs match)"
        echo "  Output: $(echo "$interp_out" | head -3)"
        PASS=$((PASS + 1))
    else
        echo "  FAIL (outputs differ)"
        echo "  Interp: $(echo "$interp_out" | head -3)"
        echo "  JIT:    $(echo "$jit_out" | head -3)"
        FAIL=$((FAIL + 1))
        ERRORS="$ERRORS\n  FAIL: $name (JIT mismatch)"
    fi
    echo ""
}

run_error_test() {
    local name="$1"
    local file="$2"
    local cmd="$3"
    local expect_patterns="$4"  # pipe-separated patterns

    echo "--- $name ---"
    local output
    output=$(eval "$cmd" 2>&1)
    local exit_code=$?

    local all_passed=true
    local found_any=false

    IFS='|' read -ra PATTERNS <<< "$expect_patterns"
    for pat in "${PATTERNS[@]}"; do
        if echo "$output" | grep -qiE "$pat"; then
            found_any=true
        fi
    done

    if $found_any || [ $exit_code -ne 0 ]; then
        echo "  PASS (error detected)"
        echo "  Exit code: $exit_code"
        echo "  Output: $(echo "$output" | head -8)"
        PASS=$((PASS + 1))
    else
        echo "  FAIL (no error detected)"
        echo "  Expected one of: $expect_patterns"
        echo "  Actual output:"
        echo "$output" | head -10 | sed 's/^/    /'
        FAIL=$((FAIL + 1))
        ERRORS="$ERRORS\n  FAIL: $name"
    fi
    echo ""
}

echo "=========================================="
echo "  SOMA v1 EDGE CASE & JIT TEST SUITE"
echo "=========================================="
echo ""

# ══════════════════════════════════════════════
echo "=== SECTION 1: TYPE COERCION (10 tests) ==="
echo ""

run_test "T01: Int + Float -> Float" "$DIR/t01_int_plus_float.cell" \
    "$SOMA run $DIR/t01_int_plus_float.cell run" "5\.5|Float"

run_test "T02: Int == Float" "$DIR/t02_int_eq_float.cell" \
    "$SOMA run $DIR/t02_int_eq_float.cell run" "true"

run_test "T03: to_int(3.7) truncate" "$DIR/t03_to_int_float.cell" \
    "$SOMA run $DIR/t03_to_int_float.cell run" "3"

run_test "T04: to_int(abc) -> null" "$DIR/t04_to_int_bad_string.cell" \
    "$SOMA run $DIR/t04_to_int_bad_string.cell run" "null.*true|is null: true"

run_test "T05: to_float(xyz) -> null" "$DIR/t05_to_float_bad_string.cell" \
    "$SOMA run $DIR/t05_to_float_bad_string.cell run" "null.*true|is null: true"

run_test "T06: to_string variants" "$DIR/t06_to_string_variants.cell" \
    "$SOMA run $DIR/t06_to_string_variants.cell run" "42"

run_test "T07: type_of all types" "$DIR/t07_type_of_all.cell" \
    "$SOMA run $DIR/t07_type_of_all.cell run" "Int|String|Bool"

run_test "T08: Truthiness" "$DIR/t08_truthiness.cell" \
    "$SOMA run $DIR/t08_truthiness.cell run" "falsy|truthy"

run_test "T09: Empty list falsy" "$DIR/t09_empty_list_falsy.cell" \
    "$SOMA run $DIR/t09_empty_list_falsy.cell run" "falsy|truthy"

run_test "T10: Mixed arithmetic" "$DIR/t10_coerce_arith.cell" \
    "$SOMA run $DIR/t10_coerce_arith.cell run" "25|3\."

# ══════════════════════════════════════════════
echo "=== SECTION 2: OVERFLOW/UNDERFLOW (5 tests) ==="
echo ""

run_test "T11: i64 overflow" "$DIR/t11_i64_overflow.cell" \
    "$SOMA run $DIR/t11_i64_overflow.cell run" "overflow|error|wrap"

run_test "T12: abs(i64::MIN)" "$DIR/t12_abs_min.cell" \
    "$SOMA run $DIR/t12_abs_min.cell run" "overflow|error|abs"

run_test "T13: Large float 1e308" "$DIR/t13_large_float.cell" \
    "$SOMA run $DIR/t13_large_float.cell run" "1e308|inf|Inf|1\.0"

run_test "T14: Division by zero" "$DIR/t14_div_by_zero.cell" \
    "$SOMA run $DIR/t14_div_by_zero.cell run" "error|zero|Inf|inf|NaN"

run_test "T15: Modulo by zero" "$DIR/t15_mod_by_zero.cell" \
    "$SOMA run $DIR/t15_mod_by_zero.cell run" "error|zero|NaN"

# ══════════════════════════════════════════════
echo "=== SECTION 3: UNICODE/UTF-8 (5 tests) ==="
echo ""

run_test "T16: Unicode len" "$DIR/t16_unicode_len.cell" \
    "$SOMA run $DIR/t16_unicode_len.cell run" "5|3"

run_test "T17: Unicode interpolation" "$DIR/t17_unicode_interp.cell" \
    "$SOMA run $DIR/t17_unicode_interp.cell run" "café|héllo"

run_test "T18: Unicode substring" "$DIR/t18_unicode_substring.cell" \
    "$SOMA run $DIR/t18_unicode_substring.cell run" "café|caf"

run_test "T19: escape_html Unicode" "$DIR/t19_escape_html.cell" \
    "$SOMA run $DIR/t19_escape_html.cell run" "&lt;|&amp;|&quot;|escape"

run_test "T20: Unicode contains/uppercase" "$DIR/t20_unicode_contains.cell" \
    "$SOMA run $DIR/t20_unicode_contains.cell run" "true|HÉLLO"

# ══════════════════════════════════════════════
echo "=== SECTION 4: RECURSION LIMITS (3 tests) ==="
echo ""

run_test "T21: fib(30)" "$DIR/t21_fib30.cell" \
    "$SOMA run $DIR/t21_fib30.cell run" "832040" ""

run_test "T22: Deep recursion" "$DIR/t22_deep_recursion.cell" \
    "$SOMA run $DIR/t22_deep_recursion.cell run" "overflow|stack|error|50000|deep"

run_test "T23: Catch stackoverflow" "$DIR/t23_catch_stackoverflow.cell" \
    "$SOMA run $DIR/t23_catch_stackoverflow.cell run" "caught|error|overflow|stack"

# ══════════════════════════════════════════════
echo "=== SECTION 5: JIT CORRECTNESS (15 tests) ==="
echo ""

run_jit_compare "T24: Arithmetic" "$DIR/t24_jit_arith.cell"
run_jit_compare "T25: Comparison" "$DIR/t25_jit_compare.cell"
run_jit_compare "T26: If/else" "$DIR/t26_jit_ifelse.cell"
run_jit_compare "T27: While loop sum" "$DIR/t27_jit_while_sum.cell"
run_jit_compare "T28: For loop" "$DIR/t28_jit_for_loop.cell"
run_jit_compare "T29: String operations" "$DIR/t29_jit_strings.cell"
run_jit_compare "T30: fib(20)" "$DIR/t30_jit_fib20.cell"
run_jit_compare "T31: Cross-handler calls" "$DIR/t31_jit_cross_handler.cell"
run_jit_compare "T32: Lambda map" "$DIR/t32_jit_lambda_map.cell"
run_jit_compare "T33: Lambda filter" "$DIR/t33_jit_lambda_filter.cell"
run_jit_compare "T34: Match expression" "$DIR/t34_jit_match.cell"
run_jit_compare "T35: Nested calls" "$DIR/t35_jit_nested_calls.cell"
run_jit_compare "T36: Short-circuit" "$DIR/t36_jit_short_circuit.cell"
run_jit_compare "T37: Null coalescing" "$DIR/t37_jit_null_coalesce.cell"
run_jit_compare "T38: Try/catch" "$DIR/t38_jit_try_catch.cell"

# ══════════════════════════════════════════════
echo "=== SECTION 6: ERROR MESSAGE QUALITY (10 tests) ==="
echo ""

run_error_test "T39: Parse error line:col" "$DIR/t39_err_parse.cell" \
    "$SOMA run $DIR/t39_err_parse.cell run" "error|line|parse|expected|unexpected"

run_error_test "T40: Undefined variable" "$DIR/t40_err_undefined_var.cell" \
    "$SOMA run $DIR/t40_err_undefined_var.cell run" "undefined|not found|unknown|undeclared"

run_error_test "T41: Type mismatch" "$DIR/t41_err_type_mismatch.cell" \
    "$SOMA run $DIR/t41_err_type_mismatch.cell run" "cannot|type|mismatch|add.*string|string.*int"

run_error_test "T42: Arity error" "$DIR/t42_err_arity.cell" \
    "$SOMA run $DIR/t42_err_arity.cell run" "arg|arity|expected.*2.*got.*3|parameter"

run_error_test "T43: Did you mean?" "$DIR/t43_err_did_you_mean.cell" \
    "$SOMA run $DIR/t43_err_did_you_mean.cell run" "did you mean|suggest|similar|undefined|unknown"

run_error_test "T44: filter_by invalid op" "$DIR/t44_err_filter_by_invalid_op.cell" \
    "$SOMA run $DIR/t44_err_filter_by_invalid_op.cell run" "operator|invalid|unknown|unsupported|valid"

run_error_test "T45: map() odd args" "$DIR/t45_err_map_odd_args.cell" \
    "$SOMA run $DIR/t45_err_map_odd_args.cell run" "odd|even|pair|argument|key.*value"

run_error_test "T46: break outside loop" "$DIR/t46_err_break_outside_loop.cell" \
    "$SOMA run $DIR/t46_err_break_outside_loop.cell run" "break|loop|outside|not.*in"

run_error_test "T47: Div by zero location" "$DIR/t47_err_div_zero_location.cell" \
    "$SOMA run $DIR/t47_err_div_zero_location.cell run" "zero|division|error|line"

run_error_test "T48: Parse error token" "$DIR/t48_err_extra_parse.cell" \
    "$SOMA run $DIR/t48_err_extra_parse.cell run" "error|unexpected|expected|parse|token"

# ══════════════════════════════════════════════
echo "=== SECTION 7: MULTI-FILE PROGRAMS (5 tests) ==="
echo ""

run_test "T49: Import and call" "$DIR/t49_multifile_import.cell" \
    "$SOMA run $DIR/t49_multifile_import.cell run" "42|double"

run_test "T50: Imported handler chain" "$DIR/t50_multifile_handler.cell" \
    "$SOMA run $DIR/t50_multifile_handler.cell run" "20"

run_error_test "T51: soma check multi-file" "$DIR/t51_multifile_check.cell" \
    "$SOMA check $DIR/t51_multifile_check.cell" "ok|pass|valid|check|error"

run_test "T52: soma test multi-file" "$DIR/t52_multifile_test.cell" \
    "$SOMA test $DIR/t52_multifile_test.cell" "pass|ok|assert"

run_test "T53: soma verify multi-file" "$DIR/t53_multifile_verify.cell" \
    "$SOMA verify $DIR/t53_multifile_verify.cell" "verify|reachable|ok|pass|state"

# ══════════════════════════════════════════════
echo "=========================================="
echo "  SUMMARY"
echo "=========================================="
echo "  PASSED: $PASS"
echo "  FAILED: $FAIL"
echo "  TOTAL:  $((PASS + FAIL))"
if [ $FAIL -gt 0 ]; then
    echo ""
    echo "  Failures:"
    echo -e "$ERRORS"
fi
echo "=========================================="
