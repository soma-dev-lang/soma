// 3.0 language and tooling: cell constants, multi-parameter lambdas, `++`,
// edition-2027 refusals (A4), `soma fmt` (A5), attestations (B5), typed
// model answers (C1).

use std::process::Command;

fn soma_in(dir: &std::path::Path, args: &[&str], env: &[(&str, &str)]) -> (String, String, i32) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_soma"));
    cmd.args(args).current_dir(dir).env_remove("SOMA_EDITION");
    for (k, v) in env { cmd.env(k, v); }
    let o = cmd.output().expect("failed to run soma");
    (String::from_utf8_lossy(&o.stdout).to_string(), String::from_utf8_lossy(&o.stderr).to_string(), o.status.code().unwrap_or(-1))
}

fn project(name: &str, files: &[(&str, &str)]) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("soma_v3l_{}_{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (f, text) in files { std::fs::write(dir.join(f), text).unwrap(); }
    dir
}

// ── A4: constants ──────────────────────────────────────────────────────

const QUEUE: &str = r#"cell Queue {
    on lim() { return LIMIT * 2 }
    const LIMIT = 3
    const greeting = "hi"
    memory {
        rows: List<Int> [persistent]
        invariant rows.size <= LIMIT
    }
    on add(x: Int) {
        require len(rows) < LIMIT else Full "at most {LIMIT} rows, {greeting}, next {LIMIT + 1}"
        rows.push(x)
        return len(rows)
    }
}
cell test T {
    rules {
        assert add(1) == 1
        assert add(2) == 2
        assert add(3) == 3
        assert_fails add(4) matching "at most 3 rows, hi, next 4"
        assert lim() == 6
    }
}
"#;

#[test]
fn a4_a_constant_is_its_literal_for_the_prover_too() {
    let d = project("const", &[("app.cell", QUEUE)]);
    let (out, err, code) = soma_in(&d, &["test", "app.cell"], &[]);
    assert_eq!(code, 0, "{out}{err}");
    assert!(out.contains("5 passed"), "{out}");
    // the bound is PROVEN from the constant: no literal repeated
    let (out, err, code) = soma_in(&d, &["verify", "--strict", "app.cell"], &[]);
    assert_eq!(code, 0, "{out}{err}");
    assert!(out.contains("invariant rows.size <= 3") && out.contains("proven"), "{out}");
}

#[test]
fn a4_a_constant_cannot_be_rebound_or_computed() {
    for (body, want) in [
        ("on f() { let LIMIT = 4  return LIMIT }", "cannot name a variable"),
        ("on f(LIMIT: Int) { return 1 }", "cannot name a parameter"),
        ("on f() { for LIMIT in [1] { }  return 1 }", "cannot name a loop variable"),
        ("on f() { LIMIT = 4  return 1 }", "cannot be assigned"),
        ("on f() { let g = (LIMIT, b) => b  return 1 }", "cannot name a lambda parameter"),
        ("const LIMIT = 4", "declared twice"),
        ("const X = 1 + 2", "takes a literal, not an expression"),
        ("const Y = len(\"a\")", "takes a literal"),
    ] {
        let d = project("constbad", &[("app.cell", &format!("cell A {{\n    const LIMIT = 3\n    {body}\n}}\n"))]);
        let (out, err, code) = soma_in(&d, &["check", "app.cell"], &[]);
        assert_eq!(code, 1, "{body}: {out}{err}");
        assert!(format!("{out}{err}").contains(want), "{body}: {out}{err}");
    }
}

// ── A4: lambdas of several parameters, `++` ────────────────────────────

#[test]
fn a4_multi_parameter_lambdas_and_concat_operator() {
    let d = project("lambda", &[("app.cell", r#"cell A {
    on f() {
        let add = (a, b) => a + b
        let vol = (x, y, z) => {
            let base = x * y
            base * z
        }
        let total = reduce([3, 1, 2], 0, (acc, x) => acc + x)
        let pairs = entries(map("a", 1, "b", 2)) |> map((k, v) => "{k}={v}")
        let one = x => x * 2
        return map("add", add(1, 2), "vol", vol(2, 3, 4), "total", total, "pairs", pairs,
                   "one", one(4), "paren", (1 + 2) * 3, "pair", add([10, 20]),
                   "join", ["x"] ++ ["y"] ++ ["z"], "text", "ab" ++ "cd")
    }
    on bad() { let add = (a, b) => a + b  return add(1, 2, 3) }
    on bad2() { let one = x => x  return one(1, 2) }
}
cell test T { rules {
    assert f() == map("add", 3, "vol", 24, "total", 6, "pairs", ["a=1", "b=2"], "one", 8, "paren", 9,
                      "pair", 30, "join", ["x", "y", "z"], "text", "abcd")
    assert_fails bad() matching "takes 2 arguments, got 3"
    assert_fails bad2() matching "several parameters"
} }
"#)]);
    let (out, err, code) = soma_in(&d, &["test", "app.cell"], &[]);
    assert_eq!(code, 0, "{out}{err}");
    assert!(out.contains("3 passed"), "{out}");
}

#[test]
fn a4_edition_2027_refuses_the_two_ambiguous_forms() {
    let src = r#"cell A {
    on g() {
        let m = map("a", 1)
        let c = m.a ?? 0 + 1
        let j = ["a"] + ["b"]
        let ok = (m.a ?? 0) + 1
        let nums = [1, 2] + [3, 4]
        return [c, j, ok, nums]
    }
}
"#;
    let d = project("ed27", &[("app.cell", src)]);
    let (out, err, code) = soma_in(&d, &["check", "app.cell"], &[]);
    assert_eq!(code, 0, "edition 2026 keeps accepting both: {out}{err}");
    let d = project("ed27b", &[("app.cell", src), ("soma.toml", "[package]\nedition = \"2027\"\n")]);
    let (out, _, code) = soma_in(&d, &["check", "app.cell"], &[]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("parentheses are required here") && out.contains("join with `++`"), "{out}");
    assert!(out.contains("2 error(s)"), "the parenthesised and numeric forms stay legal: {out}");
}

// ── A5: fmt ────────────────────────────────────────────────────────────

#[test]
fn a5_fmt_is_canonical_idempotent_and_checkable() {
    let messy = "cell A {\n  on f(x: Int) {\n\t\tlet m = map(\n\"a\", 1\n)\n      if x > 1 {\n return m\n    }   \n\n\n\n    return \"\"\"\n  raw {\n\n   text\"\"\"\n  }\n\n}\n\n";
    let d = project("fmt", &[("app.cell", messy)]);
    let (out, _, code) = soma_in(&d, &["fmt", "--check", "."], &[]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("would format"), "{out}");
    assert_eq!(std::fs::read_to_string(d.join("app.cell")).unwrap(), messy, "--check writes nothing");
    let (_, _, code) = soma_in(&d, &["fmt", "app.cell"], &[]);
    assert_eq!(code, 0);
    let formatted = std::fs::read_to_string(d.join("app.cell")).unwrap();
    assert_eq!(formatted, "cell A {\n    on f(x: Int) {\n        let m = map(\n            \"a\", 1\n        )\n        if x > 1 {\n            return m\n        }\n\n        return \"\"\"\n  raw {\n\n   text\"\"\"\n    }\n}\n");
    let (out, _, code) = soma_in(&d, &["fmt", "--check", "."], &[]);
    assert_eq!(code, 0, "{out}");
    // the same program: it still checks
    let (out, err, code) = soma_in(&d, &["check", "app.cell"], &[]);
    assert_eq!(code, 0, "{out}{err}");
}

// ── B5: attestation ────────────────────────────────────────────────────

#[test]
fn b5_an_attestation_is_deterministic_and_tamper_evident() {
    let d = project("attest", &[("app.cell", QUEUE)]);
    let (out, err, code) = soma_in(&d, &["verify", "--attest", "a.json", "app.cell"], &[]);
    assert_eq!(code, 0, "{out}{err}");
    let (_, _, _) = soma_in(&d, &["verify", "--attest", "b.json", "app.cell"], &[]);
    let a = std::fs::read_to_string(d.join("a.json")).unwrap();
    assert_eq!(a, std::fs::read_to_string(d.join("b.json")).unwrap(), "same sources, same bytes");
    let doc: serde_json::Value = serde_json::from_str(&a).unwrap();
    assert_eq!(doc["soma_attestation"], 1);
    assert_eq!(doc["sources"][0]["file"], "app.cell");
    assert!(doc["summary"]["proven"].as_u64().unwrap() >= 1, "{a}");
    assert!(doc["properties"].as_array().unwrap().iter().all(|p| matches!(p["class"].as_str(), Some("proven" | "runtime-checked" | "unproven" | "failed" | "note"))));
    let (out, err, code) = soma_in(&d, &["verify", "--check-attestation", "a.json"], &[]);
    assert_eq!(code, 0, "{out}{err}");
    assert!(out.contains("ATTESTATION CONFIRMED"), "{out}");
    // an edited verdict
    std::fs::write(d.join("forged.json"), a.replace("\"strict\": false", "\"strict\": true")).unwrap();
    let (_, err, code) = soma_in(&d, &["verify", "--check-attestation", "forged.json"], &[]);
    assert_eq!(code, 1);
    assert!(err.contains("ATTESTATION REJECTED"), "{err}");
    // a source changed after the fact
    std::fs::write(d.join("app.cell"), QUEUE.replace("const LIMIT = 3", "const LIMIT = 4")).unwrap();
    let (_, err, code) = soma_in(&d, &["verify", "--check-attestation", "a.json"], &[]);
    assert_eq!(code, 1);
    assert!(err.contains("app.cell changed since it was attested"), "{err}");
}

// ── C1: typed model answers ────────────────────────────────────────────

#[test]
fn c1_think_json_as_a_declared_type() {
    let d = project("think", &[("app.cell", r#"cell type Decision {
    variants { AutoPay { amount: Int }  Reject { reason: String }  Skip }
}
cell type Invoice { variants { Invoice { vendor: String, cents: Int } } }
cell agent Clerk {
    on decide(text: String) {
        let d = think_json("Decide on: {text}", map("as", "Decision", "max_tokens", 200))
        return match d {
            AutoPay { amount } -> "pay {amount}"
            Reject { reason } -> "no: {reason}"
            Skip -> "skip"
        }
    }
    on read(text: String) {
        let inv = think_json("Extract: {text}", map("as", "Invoice", "max_tokens", 200))
        return inv.vendor + ":" + to_string(inv.cents)
    }
}
cell test T {
    rules {
        mock think "{\"variant\": \"AutoPay\", \"amount\": 200}"
        assert decide("x") == "pay 200"
        mock think "{\"variant\": \"Skip\"}"
        assert decide("x") == "skip"
        mock think "{\"variant\": \"AutoPay\", \"amount\": \"lots\"}"
        assert_fails decide("x") matching "json"
        mock think "{\"variant\": \"Bribe\", \"amount\": 1}"
        assert_fails decide("x") matching "is not a variant of Decision"
        mock think "{\"variant\": \"AutoPay\", \"amount\": 1, \"note\": \"x\"}"
        assert_fails decide("x") matching "has no field 'note'"
        mock think "{\"vendor\": \"acme\", \"cents\": 1250}"
        assert read("x") == "acme:1250"
        mock think "{\"vendor\": \"acme\"}"
        assert_fails read("x") matching "field 'cents' is missing"
    }
}
"#)]);
    let (out, err, code) = soma_in(&d, &["test", "app.cell"], &[]);
    assert_eq!(code, 0, "{out}{err}");
    assert!(out.contains("7 passed"), "{out}");
}

/// The answer's type is known to check: a field typo, an unknown type name.
#[test]
fn c1_the_typed_answer_is_checked_statically() {
    let d = project("thinkbad", &[("app.cell", r#"cell type Invoice { variants { Invoice { vendor: String, cents: Int } } }
cell agent Clerk {
    on read(text: String) {
        let inv = think_json("Extract: {text}", map("as", "Invoice", "max_tokens", 200))
        return inv.vendr
    }
    on other(text: String) {
        return think_json("x", map("as", "Invoce", "max_tokens", 200))
    }
    on plain(text: String) {
        return think("x", map("as", "Invoice", "max_tokens", 200))
    }
}
"#)]);
    let (out, _, code) = soma_in(&d, &["check", "app.cell"], &[]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("record type 'Invoice' has no field 'vendr' (did you mean 'vendor'?)"), "{out}");
    assert!(out.contains("`as` names \"Invoce\" (did you mean \"Invoice\"?)"), "{out}");
    assert!(out.contains("\"as\" is not a think() option"), "{out}");
}

// ── B3: transition sources in the proof ────────────────────────────────

const ORDER: &str = r#"cell Order {
    state life {
        initial: placed
        placed -> held
        held -> paid
        held -> void
        paid -> shipped
    }
    on hold(id: String) {
        require get_status(id) == "placed" else NotPlaced
        transition(id, "held")
    }
    on pay(id: String) {
        let s = get_status(id)
        if s != "held" { return "no" }
        transition(id, "paid")
        return "ok"
    }
    on finish(id: String) {
        match get_status(id) {
            "held" -> transition(id, "void")
            "paid" -> transition(id, "shipped")
            _ -> ()
        }
    }
    on blind(id: String) { transition(id, "held") }
}
"#;

#[test]
fn b3_the_source_a_handler_established_is_in_the_proof() {
    let d = project("sources", &[("app.cell", ORDER)]);
    let (out, err, code) = soma_in(&d, &["verify", "app.cell"], &[]);
    assert_eq!(code, 0, "{out}{err}");
    for edge in ["handler `hold` ⟶ {placed → held}", "handler `pay` ⟶ {held → paid}", "held → void", "paid → shipped"] {
        assert!(out.contains(edge), "missing {edge}: {out}");
    }
    // only the handler that says nothing about where it starts is left blind
    assert!(out.contains("transition sources: `blind` calls"), "{out}");
}

#[test]
fn b3_a_move_no_known_source_allows_is_refused() {
    let d = project("badedge", &[("app.cell", &ORDER.replace("if s != \"held\" { return \"no\" }", "if s != \"placed\" { return \"no\" }"))]);
    let (out, _, code) = soma_in(&d, &["verify", "app.cell"], &[]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("moves an instance it knows is `placed` to `paid`") && out.contains("no edge placed → paid"), "{out}");
    assert!(out.contains("VERIFY FAILED"), "{out}");
}

/// Paths are joined without their data conditions: two exclusive `if`s are
/// not a move from the first one's target.
#[test]
fn b3_exclusive_branches_are_not_refused() {
    let d = project("exclusive", &[("app.cell", r#"cell Motion {
    state motion { initial: voting  voting -> passed  voting -> failed }
    on resolve(id: String, a: Int, n: Int) {
        if get_status(id) != "voting" { return "closed" }
        if a > n { transition(id, "passed") }
        if a <= n { transition(id, "failed") }
        return get_status(id)
    }
    on twice(id: String) {
        let r = try { transition(id, "passed") }
        transition(id, "failed")
    }
    on after_call(id: String) {
        require get_status(id) == "voting" else Closed
        helper(id)
        transition(id, "failed")
    }
    on helper(id: String) { return id }
}
"#)]);
    let (out, err, code) = soma_in(&d, &["verify", "app.cell"], &[]);
    assert_eq!(code, 0, "{out}{err}");
    assert!(out.contains("VERIFY OK"), "{out}");
    // a call into another handler forgets what was known
    assert!(out.contains("`after_call`"), "{out}");
}

// ── A4, the rest: std.f(), one vocabulary for locals, get_status ───────

const VOCAB: &str = r#"cell A {
    face {
        signal go() -> List
        signal len(a: Int, b: Int) -> Int
        signal uppercase(s: String) -> String
        signal shout(s: String) -> String
        signal st(id: String) -> Any
        signal m() -> Map
        signal pay(id: String) -> String
    }
    state order { initial: draft  draft -> paid }
    on len(a: Int, b: Int) { return a + b }
    on uppercase(s: String) { return "mine:{s}" }
    on shout(s: String) { return "{uppercase(s)} {std.uppercase(s)} {std.len(s)}" }
    on go() {
        let xs = [1]
        xs.push(2)
        let ys = xs.push(9)
        let k = len(1, 2)
        let n = std.len("abc")
        return [xs, ys, n, k, [5, 6] |> std.map(x => x * 2)]
    }
    on st(id: String) { return get_status(id) }
    on pay(id: String) {
        transition(id, "paid")
        return get_status(id)
    }
    on m() {
        let d = map("a", 1)
        d.set("b", 2)
        d["c"] = 3
        d.delete("a")
        return d
    }
}

cell test T {
    rules {
        assert go() == [[1, 2], [1, 2, 9], 3, 3, [10, 12]]
        assert m() == map("b", 2, "c", 3)
        assert shout("ab") == "mine:ab AB 2"
        assert pay("o1") == "paid"
    }
}
"#;

#[test]
fn a4_std_reaches_the_builtin_and_locals_take_the_slot_methods() {
    let d = project("vocab", &[("app.cell", VOCAB)]);
    // `xs.push(x)`, `m.set(k, v)`, `m.delete(k)` as statements write the local;
    // `std.len` is the builtin beside a handler `len`
    let (out, err, code) = soma_in(&d, &["check", "app.cell"], &[]);
    assert_eq!(code, 0, "{out}{err}");
    assert!(!format!("{out}{err}").contains("is a memory-slot method"), "{out}{err}");
    assert!(!format!("{out}{err}").contains("is discarded"), "{out}{err}");
    let (out, err, code) = soma_in(&d, &["test", "app.cell"], &[]);
    assert_eq!(code, 0, "{out}{err}");

    // `std.` names a builtin; `std` is not also a local
    let bad = VOCAB.replace("let n = std.len(\"abc\")", "let n = std.lenn(\"abc\")");
    std::fs::write(d.join("bad.cell"), bad).unwrap();
    let (out, err, code) = soma_in(&d, &["check", "bad.cell"], &[]);
    assert_ne!(code, 0);
    assert!(format!("{out}{err}").contains("there is no builtin `lenn` (did you mean `std.len`?)"), "{out}{err}");
    let bad = VOCAB.replace("let n = std.len(\"abc\")", "let std = 2.5\n        let n = std.round(1)");
    std::fs::write(d.join("bad.cell"), bad).unwrap();
    let (out, err, code) = soma_in(&d, &["check", "bad.cell"], &[]);
    assert_ne!(code, 0);
    assert!(format!("{out}{err}").contains("`std` is also a local here"), "{out}{err}");
}

#[test]
fn a4_edition_2027_a_handler_name_is_the_handler_and_an_unknown_id_has_no_status() {
    let src = VOCAB.replace("let n = std.len(\"abc\")", "let n = len(\"abc\")\n        push(xs, 4)");
    let d = project("ed2027b", &[("app.cell", &src)]);
    // 2026: the arity picks the builtin, a dropped result is a warning
    let (out, err, code) = soma_in(&d, &["check", "app.cell"], &[]);
    let all = format!("{out}{err}");
    assert_eq!(code, 0, "{all}");
    assert!(all.contains("resolves to the BUILTIN len()") && all.contains("write `std.len(…)`"), "{all}");
    assert!(all.contains("the result of `push` is discarded"), "{all}");
    // 2027: both are errors — under `soma test` too, which reads the edition itself
    for cmd in ["check", "test"] {
        let (out, err, code) = soma_in(&d, &[cmd, "app.cell"], &[("SOMA_EDITION", "2027")]);
        let all = format!("{out}{err}");
        assert_ne!(code, 0, "{cmd}: {all}");
        assert!(all.contains("edition 2027: a bare call of a name the program defines is its handler"), "{cmd}: {all}");
        assert!(all.contains("is discarded") && all.contains("edition 2027: an error"), "{cmd}: {all}");
    }

    // get_status of an id that was never transitioned
    let d = project("status", &[("app.cell", VOCAB)]);
    let (out, _, _) = soma_in(&d, &["run", "app.cell", "st", "nope"], &[]);
    assert!(out.contains("draft"), "2026 answers the initial state: {out}");
    std::fs::write(d.join("soma.toml"), "[package]\nname = \"a\"\nedition = \"2027\"\n").unwrap();
    let (out, err, code) = soma_in(&d, &["run", "app.cell", "st", "nope"], &[]);
    assert_eq!(code, 0, "{out}{err}");
    assert!(!out.contains("draft"), "2027 answers (): {out}");
    // …and once transitioned it has one
    let (out, err, code) = soma_in(&d, &["run", "app.cell", "pay", "o9"], &[]);
    assert_eq!(code, 0, "{out}{err}");
    assert!(out.contains("paid"), "{out}");
    // the migration names the one change no rewrite can make
    std::fs::write(d.join("soma.toml"), "[package]\nname = \"a\"\n").unwrap();
    let (out, err, _) = soma_in(&d, &["fix", "--edition", "2027", "app.cell"], &[]);
    assert!(format!("{out}{err}").contains("2 call(s) of get_status(id)"), "{out}{err}");
}
