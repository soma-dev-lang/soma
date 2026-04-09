//! Backend-equivalence harness — `interpreter ≡ bytecode VM` on the
//! intersection corpus.
//!
//! The README and `docs/SEMANTICS.md` §2 claim *backend equivalence*
//! between the interpreter and the bytecode VM. As of v2.2 the bytecode
//! VM is feature-incomplete (the `--jit` flag itself prints
//! "does not support all features yet"). This harness defines the
//! intersection of features both backends *do* implement and asserts
//! bit-equal output for a curated corpus exercising it.
//!
//! Failure modes this catches:
//!   - Either backend miscomputing arithmetic, control flow, or BigInt
//!   - The two backends disagreeing on the same input
//!   - A regression where the VM was previously correct on a program
//!     and now isn't
//!
//! What's NOT in scope:
//!   - Features the VM does not yet implement (string interpolation,
//!     pipes with map/filter, complex pattern matching, …). These are
//!     listed at the bottom of this file as `KNOWN_GAPS`.
//!   - The `[native]` codegen — that's a separate harness because it
//!     emits Rust source via cdylib, not interpretation.
//!
//! This harness is the executable witness for the equivalence
//! conjecture in `docs/SEMANTICS.md` §2. As long as it's green, the
//! conjecture holds on the intersection corpus.
//!
//!   cargo test --test equivalence

use std::io::Write;
use std::process::Command;

/// One equivalence test case.
struct Case {
    name: &'static str,
    /// Soma source. The handler must use explicit `return` so the VM,
    /// which doesn't yet support implicit-return-of-last-expression,
    /// produces the same output as the interpreter.
    source: &'static str,
    /// CLI args to pass after the file path. Use `--signal name` to
    /// pick a non-default handler.
    args: &'static [&'static str],
    /// Trimmed expected stdout from BOTH backends. Lets us catch
    /// regressions in either backend, not just disagreements.
    expected: &'static str,
}

const CASES: &[Case] = &[
    Case {
        name: "constant_return",
        source: "cell P { on run() { return 42 } }",
        args: &[],
        expected: "42",
    },
    Case {
        name: "arithmetic_precedence",
        source: "cell P { on run() { return 1 + 2 * 3 } }",
        args: &[],
        expected: "7",
    },
    Case {
        name: "local_then_arithmetic",
        source: "cell P { on run() { let x = 10\n let y = 5\n return x + y * 2 } }",
        args: &[],
        expected: "20",
    },
    Case {
        name: "if_then_branch",
        source: "cell P { on run(n: Int) { if n > 5 { return 100 } return 0 } }",
        args: &["7"],
        expected: "100",
    },
    Case {
        name: "if_else_branch",
        source: "cell P { on run(n: Int) { if n > 5 { return 100 } return 0 } }",
        args: &["3"],
        expected: "0",
    },
    Case {
        name: "for_range_sum",
        source: "cell P { on run(n: Int) { let total = 0\n for i in range(0, n) { total = total + i }\n return total } }",
        args: &["10"],
        expected: "45",
    },
    Case {
        name: "recursive_factorial_small",
        source: "cell P { on fact(n: Int) { if n <= 1 { return 1 }\n return n * fact(n - 1) } }",
        args: &["--signal", "fact", "5"],
        expected: "120",
    },
    Case {
        // 25! overflows i64 (max ≈ 9.2e18); 25! ≈ 1.55e25.
        // Both backends MUST auto-promote to BigInt.
        name: "factorial_25_bigint_promotion",
        source: "cell P { on fact(n: Int) { if n <= 1 { return 1 }\n return n * fact(n - 1) } }",
        args: &["--signal", "fact", "25"],
        expected: "15511210043330985984000000",
    },
    Case {
        name: "bool_literal",
        source: "cell P { on run() { return true } }",
        args: &[],
        expected: "true",
    },
    Case {
        name: "negative_arithmetic",
        source: "cell P { on run() { return 0 - 17 } }",
        args: &[],
        expected: "-17",
    },
    Case {
        name: "modulo",
        source: "cell P { on run() { return 17 % 5 } }",
        args: &[],
        expected: "2",
    },
    Case {
        name: "list_len",
        source: "cell P { on run() { let xs = list(1, 2, 3, 4, 5)\n return len(xs) } }",
        args: &[],
        expected: "5",
    },
    Case {
        name: "map_dot_access",
        source: "cell P { on run() { let m = map(\"a\", 1, \"b\", 2)\n return m.a + m.b } }",
        args: &[],
        expected: "3",
    },
    Case {
        name: "string_concat",
        source: "cell P { on run() { return concat(\"hello\", \"world\") } }",
        args: &[],
        expected: "helloworld",
    },
    Case {
        name: "two_arg_handler",
        source: "cell P { on add(a: Int, b: Int) { return a + b } }",
        args: &["--signal", "add", "3", "4"],
        expected: "7",
    },
    Case {
        name: "comparison_chain",
        source: "cell P { on run(n: Int) { if n > 0 { if n < 100 { return 1 } return 2 } return 0 } }",
        args: &["50"],
        expected: "1",
    },
];

/// Run `soma` from the test binary's working directory and capture stdout.
fn soma_run(args: &[&str]) -> (String, String, i32) {
    let exe = "./target/release/soma";
    let output = Command::new(exe)
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("failed to run soma — did you `cargo build --release`?");
    (
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.code().unwrap_or(-1),
    )
}

/// Strip the VM's deprecation warnings so we can compare clean output.
/// The VM prints two lines on every invocation; the actual return value
/// is on the LAST non-empty line.
fn last_value(s: &str) -> String {
    s.lines()
        .map(|l| l.trim_end())
        .filter(|l| !l.is_empty())
        .filter(|l| !l.starts_with("warning:"))
        .filter(|l| !l.starts_with("note:"))
        .filter(|l| !l.starts_with("  on simulate"))
        .filter(|l| !l.starts_with("  See:"))
        .last()
        .unwrap_or("")
        .to_string()
}

fn write_temp(name: &str, source: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("soma_eq_{}", name));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("app.cell");
    let mut f = std::fs::File::create(&path).unwrap();
    f.write_all(source.as_bytes()).unwrap();
    path
}

fn run_case(case: &Case) -> Result<(String, String), String> {
    let path = write_temp(case.name, case.source);
    let path_str = path.to_string_lossy().to_string();

    // ── Interpreter ─────────────────────────────────────────────
    let mut interp_args = vec!["run", path_str.as_str()];
    interp_args.extend_from_slice(case.args);
    let (out_i, err_i, code_i) = soma_run(&interp_args);
    if code_i != 0 {
        return Err(format!(
            "interpreter exited {}: stdout=`{}` stderr=`{}`",
            code_i,
            out_i.trim(),
            err_i.trim()
        ));
    }
    let val_i = last_value(&out_i);

    // ── Bytecode VM ─────────────────────────────────────────────
    let mut vm_args = vec!["run", "--jit", path_str.as_str()];
    vm_args.extend_from_slice(case.args);
    let (out_v, err_v, code_v) = soma_run(&vm_args);
    if code_v != 0 {
        return Err(format!(
            "VM exited {}: stdout=`{}` stderr=`{}`",
            code_v,
            out_v.trim(),
            err_v.trim()
        ));
    }
    let val_v = last_value(&out_v);

    Ok((val_i, val_v))
}

#[test]
fn equivalence_intersection_corpus() {
    let mut failed = Vec::new();

    for case in CASES {
        match run_case(case) {
            Ok((interp, vm)) => {
                if interp != case.expected {
                    failed.push(format!(
                        "{}: interpreter returned `{}`, expected `{}`",
                        case.name, interp, case.expected
                    ));
                } else if vm != case.expected {
                    failed.push(format!(
                        "{}: VM returned `{}`, expected `{}` (interpreter agreed)",
                        case.name, vm, case.expected
                    ));
                } else if interp != vm {
                    // Defensive: should be unreachable since both equal `expected`,
                    // but document the divergence form for future readers.
                    failed.push(format!(
                        "{}: backends disagree — interpreter `{}`, VM `{}`",
                        case.name, interp, vm
                    ));
                } else {
                    eprintln!("  ✓ {} ({})", case.name, interp);
                }
            }
            Err(e) => failed.push(format!("{}: {}", case.name, e)),
        }
    }

    if !failed.is_empty() {
        panic!(
            "\n{} of {} cases failed:\n  {}\n",
            failed.len(),
            CASES.len(),
            failed.join("\n  ")
        );
    }
}

// ── Known gaps in the bytecode VM ──────────────────────────────────
//
// Features that exist in the interpreter but the VM does not yet
// implement, intentionally excluded from the corpus above. These are
// the gaps the equivalence claim is honestly scoped around.
//
// 1. **String interpolation** — `"hello {name}"`. The VM's note line
//    explicitly disclaims it.
// 2. **Implicit return of last expression** — the VM returns Unit if
//    a handler ends in an expression statement without `return`.
//    Every case above uses explicit `return`.
// 3. **Pipes with lambdas** — `xs |> filter(x => x > 0)`. Untested
//    in the VM and likely incomplete.
// 4. **Pattern matching with map destructure** — used heavily by
//    `on request(method, path, body) { match req { {method: "GET"...} } }`.
//    Untested.
// 5. **`delegate("Cell", "signal", args)`** — cross-cell calls in the
//    VM are unverified by this harness.
// 6. **`transition(id, "state")` and the state machine plumbing**
//    rely on storage backends that the VM may not wire up the same
//    way as the interpreter — out of scope.
// 7. **`think()` and other LLM builtins** — not in the intersection.
//
// When any of these gaps closes (i.e. the VM grows to support a
// feature), add a case here that exercises it. The harness must grow
// monotonically with the VM's capability.
