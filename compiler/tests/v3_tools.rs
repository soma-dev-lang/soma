// 3.0 tooling: edition migration (D1), the split agent guide (D2), recorded
// model replies (C4), evals (C6), the language server (A6), the backend
// fuzzer (B1).

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};

fn soma_in(dir: &std::path::Path, args: &[&str], env: &[(&str, &str)]) -> (String, String, i32) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_soma"));
    cmd.args(args).current_dir(dir).env_remove("SOMA_EDITION").env_remove("SOMA_LLM_MOCK");
    for (k, v) in env { cmd.env(k, v); }
    let o = cmd.output().expect("failed to run soma");
    (String::from_utf8_lossy(&o.stdout).to_string(), String::from_utf8_lossy(&o.stderr).to_string(), o.status.code().unwrap_or(-1))
}

fn project(name: &str, files: &[(&str, &str)]) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("soma_v3t_{}_{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (f, text) in files { std::fs::write(dir.join(f), text).unwrap(); }
    dir
}

// ── D1: soma fix --edition ─────────────────────────────────────────────

#[test]
fn d1_fix_edition_rewrites_what_is_mechanical_and_names_the_rest() {
    let d = project("migrate", &[("app.cell", r#"cell A {
    memory { counts: Map<String, Int> [persistent] }
    on g(k: String) {
        let m = map("a", 1)
        let c = m.a ?? 0 + 1
        let e = m.b ?? m.a ?? 2 * 3
        let j = ["a"] + ["b"]
        let n = [1, 2] + [3, 4]
        return [c, e, j, n]
    }
    on bump(k: String) {
        counts.set(k, counts.get(k) + 1)
    }
}
"#), ("soma.toml", "[agent]\nmock = \"echo\"\n")]);
    let (before, _, _) = soma_in(&d, &["run", "--fresh", "app.cell", "g", "x"], &[]);
    let (out, _, code) = soma_in(&d, &["fix", "--edition", "2027", "app.cell"], &[]);
    // the absent value is a decision, not a rewrite: it is reported
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("written with its parentheses") && out.contains("rewritten `xs ++ ys`") && out.contains("edition = \"2027\""), "{out}");
    assert!(out.contains("left for you under edition 2027 (1 error(s))") && out.contains("may be absent"), "{out}");
    let src = std::fs::read_to_string(d.join("app.cell")).unwrap();
    assert!(src.contains("m.a ?? (0 + 1)") && src.contains("m.b ?? m.a ?? (2 * 3)") && src.contains("[\"a\"] ++ [\"b\"]"), "{src}");
    assert!(src.contains("[1, 2] + [3, 4]"), "numeric lists keep `+`: {src}");
    let toml = std::fs::read_to_string(d.join("soma.toml")).unwrap();
    assert!(toml.starts_with("[package]\nedition = \"2027\"\n") && toml.contains("[agent]\nmock = \"echo\""), "{toml}");
    // the rewritten handler answers what it answered
    let (after, _, _) = soma_in(&d, &["run", "--fresh", "app.cell", "g", "x"], &[("SOMA_EDITION", "2026")]);
    assert_eq!(before.lines().last(), after.lines().last());
    // a second run has nothing left to rewrite
    let (out, _, _) = soma_in(&d, &["fix", "--edition", "2027", "app.cell"], &[]);
    assert!(out.contains("nothing to rewrite"), "{out}");
}

// ── D2: the agent guide in parts ───────────────────────────────────────

#[test]
fn d2_the_agent_guide_is_split_and_every_part_ships() {
    let d = project("docs", &[]);
    let (agent, _, _) = soma_in(&d, &["docs", "agent"], &[]);
    assert!(agent.lines().count() < 720, "the core guide grew back: {} lines", agent.lines().count());
    assert!(!agent.contains("Soma 2.8.4 corrects"), "release notes belong in `soma docs releases`");
    for (topic, needle) in [("agents", "HORDES"), ("native", "buffer(n) -> Buf"), ("releases", "Soma 2.8.4 corrects")] {
        let (out, _, code) = soma_in(&d, &["docs", topic], &[]);
        assert_eq!(code, 0);
        assert!(out.contains(needle), "soma docs {topic} lacks {needle:?}");
        assert!(agent.contains(&format!("soma docs {topic}")) || agent.contains(&format!("|{topic}")), "the core guide does not point to `soma docs {topic}`");
    }
    let (all, _, _) = soma_in(&d, &["docs", "all"], &[]);
    assert!(all.contains("HORDES") && all.contains("buffer(n) -> Buf") && all.contains("Soma 2.8.4 corrects"));
}

// ── C4: recorded model replies ─────────────────────────────────────────

#[test]
fn c4_replay_hands_back_the_recorded_model_answers() {
    let d = project("replay", &[("app.cell", r#"cell agent Bot {
    on ask(q: String) {
        let a = think("Q: {q}", map("max_tokens", 50))
        let b = think_json("J: {q}", map("max_tokens", 50))
        return map("a", a, "b", b)
    }
}
"#)]);
    let (out, err, code) = soma_in(&d, &["run", "--record", "--fresh", "app.cell", "ask", "hi"], &[("SOMA_LLM_MOCK", "fixed:{\"k\": 1}")]);
    assert_eq!(code, 0, "{out}{err}");
    assert_eq!(std::fs::read_to_string(d.join("app.somalog.replies")).unwrap().lines().count(), 2);
    // the model would answer something else now: the replay does not ask it
    let (out, err, code) = soma_in(&d, &["replay", "app.cell"], &[("SOMA_LLM_MOCK", "fixed:{\"k\": 2}")]);
    assert_eq!(code, 0, "{out}{err}");
    assert!(out.contains("2 recorded answer(s) replayed") && out.contains("1 ok, 0 diverged"), "{out}");
    let (out, _, code) = soma_in(&d, &["replay", "app.cell", "--live"], &[("SOMA_LLM_MOCK", "fixed:{\"k\": 2}")]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("asked again (--live)") && out.contains("0 ok, 1 diverged"), "{out}");
}

// ── C6: evals ──────────────────────────────────────────────────────────

const ROUTER: &str = r#"cell Router {
    on classify(text: String) {
        if contains(lowercase(text), "refund") { return "billing" }
        if contains(lowercase(text), "password") { return "account" }
        return "other"
    }
    on judge(input: String, answer: String, expected: String) { return len(answer) > 0 }
}
"#;
const CASES: &str = "{\"input\": \"Refund please\", \"expected\": \"billing\", \"name\": \"refund\"}\n{\"input\": \"lost my PASSWORD\", \"expected\": \"account\"}\n{\"input\": \"hello {x} \\\"q\\\"\", \"expected\": \"billing\", \"name\": \"greeting\"}\n";

#[test]
fn c6_eval_grades_a_handler_and_names_regressions() {
    let d = project("eval", &[("app.cell", ROUTER), ("cases.jsonl", CASES)]);
    let (out, err, code) = soma_in(&d, &["eval", "app.cell", "--handler", "classify", "--dataset", "cases.jsonl", "--threshold", "0.6"], &[]);
    assert_eq!(code, 0, "{out}{err}");
    assert!(out.contains("2 / 3 cases pass") && out.contains("greeting — answered other, expected billing") && out.contains("EVAL OK"), "{out}");
    // the default threshold is every case
    let (out, _, code) = soma_in(&d, &["eval", "app.cell", "--handler", "classify", "--dataset", "cases.jsonl"], &[]);
    assert_eq!(code, 1, "{out}");
    // a change that breaks a case that passed
    std::fs::write(d.join("app.cell"), ROUTER.replace("return \"billing\"", "return \"bill\"")).unwrap();
    let (out, _, code) = soma_in(&d, &["eval", "app.cell", "--handler", "classify", "--dataset", "cases.jsonl", "--threshold", "0.6", "--json"], &[]);
    assert_eq!(code, 1, "{out}");
    let doc: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(doc["passed"], 1);
    assert_eq!(doc["regressions"], serde_json::json!(["refund"]));
    // your own grader
    let (out, err, code) = soma_in(&d, &["eval", "app.cell", "--handler", "classify", "--dataset", "cases.jsonl", "--grade", "handler:Router.judge"], &[]);
    assert_eq!(code, 0, "{out}{err}");
    // nothing is left beside the program but the stored runs
    let names: Vec<String> = std::fs::read_dir(&d).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().to_string()).collect();
    assert!(!names.iter().any(|n| n.contains("eval-")), "{names:?}");
    let (_, err, code) = soma_in(&d, &["eval", "app.cell", "--handler", "classify", "--dataset", "cases.jsonl", "--grade", "vibes"], &[]);
    assert_eq!(code, 1);
    assert!(err.contains("exact, contains or handler:Cell.judge"), "{err}");
}

// ── A6: language server ────────────────────────────────────────────────

fn lsp_send(stdin: &mut impl Write, msg: serde_json::Value) {
    let body = msg.to_string();
    write!(stdin, "Content-Length: {}\r\n\r\n{}", body.len(), body).unwrap();
    stdin.flush().unwrap();
}

fn lsp_recv(out: &mut impl BufRead) -> serde_json::Value {
    let mut len = 0usize;
    loop {
        let mut line = String::new();
        out.read_line(&mut line).unwrap();
        let line = line.trim_end();
        if line.is_empty() { break; }
        if let Some(v) = line.strip_prefix("Content-Length:") { len = v.trim().parse().unwrap(); }
    }
    let mut body = vec![0u8; len];
    out.read_exact(&mut body).unwrap();
    serde_json::from_slice(&body).unwrap()
}

#[test]
fn a6_lsp_publishes_check_diagnostics_and_formats() {
    let text = "cell A {\n  on f(s: String) { let n = s * 2\n return nope(n) }\n}\n";
    let d = project("lsp", &[("bad.cell", text)]);
    let uri = format!("file://{}", d.join("bad.cell").display());
    let mut child = Command::new(env!("CARGO_BIN_EXE_soma")).arg("lsp").stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut out = BufReader::new(child.stdout.take().unwrap());
    lsp_send(&mut stdin, serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}));
    assert_eq!(lsp_recv(&mut out)["result"]["capabilities"]["documentFormattingProvider"], true);
    lsp_send(&mut stdin, serde_json::json!({"jsonrpc": "2.0", "method": "textDocument/didOpen", "params": {"textDocument": {"uri": uri, "text": text}}}));
    let diags = lsp_recv(&mut out);
    assert_eq!(diags["method"], "textDocument/publishDiagnostics");
    let list = diags["params"]["diagnostics"].as_array().unwrap();
    assert_eq!(list.len(), 2, "{diags}");
    assert!(list.iter().any(|x| x["range"]["start"]["line"] == 1 && x["message"].as_str().unwrap().contains("cannot multiply a String and an Int")), "{diags}");
    assert!(list.iter().all(|x| x["code"].as_str().unwrap().starts_with('E') && x["source"] == "soma"));
    lsp_send(&mut stdin, serde_json::json!({"jsonrpc": "2.0", "id": 2, "method": "textDocument/formatting", "params": {"textDocument": {"uri": uri}}}));
    assert_eq!(lsp_recv(&mut out)["result"][0]["newText"], "cell A {\n    on f(s: String) { let n = s * 2\n        return nope(n) }\n}\n");
    lsp_send(&mut stdin, serde_json::json!({"jsonrpc": "2.0", "id": 3, "method": "textDocument/hover", "params": {}}));
    assert_eq!(lsp_recv(&mut out)["error"]["code"], -32601);
    lsp_send(&mut stdin, serde_json::json!({"jsonrpc": "2.0", "id": 4, "method": "shutdown"}));
    lsp_recv(&mut out);
    lsp_send(&mut stdin, serde_json::json!({"jsonrpc": "2.0", "method": "exit"}));
    assert_eq!(child.wait().unwrap().code(), Some(0));
}

// ── B1: the differential fuzzer runs ───────────────────────────────────

/// A small campaign: the generator's programs pass check, both backends run
/// them, and no value differs. (The campaigns that count are run with
/// `tools/diff_fuzz.py --files N`; this keeps the tool from rotting.)
#[test]
fn b1_the_backend_fuzzer_finds_no_wrong_answer_on_a_small_campaign() {
    if Command::new("python3").arg("--version").output().is_err() || Command::new("cargo").arg("--version").output().is_err() {
        eprintln!("skipped: needs python3 and cargo");
        return;
    }
    let tool = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../tools/diff_fuzz.py");
    let o = Command::new("python3").arg(tool).args(["--files", "2", "--funcs", "10", "--inputs", "8", "--seed", "42", "--json"])
        .env("SOMA", env!("CARGO_BIN_EXE_soma")).output().unwrap();
    let doc: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&o.stderr)));
    assert_eq!(doc["comparisons"], 160);
    assert_eq!(doc["divergences"].as_array().unwrap().len(), 0, "{doc}");
    assert_eq!(doc["broken"].as_array().unwrap().len(), 0, "{doc}");
}

// ── B6: untrusted values ───────────────────────────────────────────────

const FLOW: &str = r#"cell agent Flow {
    state s { initial: open  open -> approved  open -> rejected }
    on bad(id: String, text: String) {
        let next = think("approve or reject: {text}", map("max_tokens", 10))
        transition(id, next)
    }
    on good(id: String, text: String) {
        let next = think("approve or reject: {text}", map("max_tokens", 10))
        match next {
            "approved" -> transition(id, "approved")
            _ -> transition(id, "rejected")
        }
    }
    on good2(id: String, text: String) {
        let next = think("x", map("max_tokens", 10))
        require contains(["approved", "rejected"], next) else BadState
        transition(id, next)
    }
    on fetch(text: String) {
        let d = think_json("which url? {text}", map("max_tokens", 50))
        let u = d.url
        return http_get(u)
    }
    on fetch2(q: String) {
        let page = http_get("https://api.example.com/next?q={q}")
        return http_get("https://api.example.com/{page.body}")
    }
    on store(text: String) {
        let s = think("summarize {text}", map("max_tokens", 50))
        return "stored {s}"
    }
}
"#;

#[test]
fn b6_a_model_answer_may_not_decide_without_a_check() {
    let d = project("taint", &[("app.cell", FLOW)]);
    let (out, _, code) = soma_in(&d, &["check", "--json", "app.cell"], &[]);
    assert_eq!(code, 0, "edition 2026 warns: {out}");
    let doc: serde_json::Value = serde_json::from_str(&out).unwrap();
    let lines: Vec<i64> = doc["warnings"].as_array().unwrap().iter()
        .filter(|w| w["message"].as_str().unwrap().contains(" decides "))
        .map(|w| w["line"].as_i64().unwrap()).collect();
    // bad (the state), fetch (the URL, through a field), fetch2 (the URL, through an interpolation)
    assert_eq!(lines, vec![5, 22, 26], "{out}");
    assert!(out.contains("comes from think()") && out.contains("comes from think_json()") && out.contains("comes from http_get()"), "{out}");
    let (out, _, code) = soma_in(&d, &["check", "--json", "app.cell"], &[("SOMA_EDITION", "2027")]);
    assert_eq!(code, 1);
    let doc: serde_json::Value = serde_json::from_str(&out).unwrap();
    let kinds: Vec<&str> = doc["errors"].as_array().unwrap().iter().map(|e| e["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, vec!["untrusted_value"; 3], "{out}");
}

// ── C7: OpenTelemetry ──────────────────────────────────────────────────

#[test]
fn c7_an_invocation_is_one_otlp_span_without_content_by_default() {
    let d = project("otel", &[("app.cell", r#"cell agent Bot {
    on ask(q: String) { return think("SECRET-PROMPT {q}", map("max_tokens", 50)) }
    on boom() { fail("not_found", "nope") }
}
"#)]);
    let env = [("SOMA_OTEL_FILE", "spans.jsonl"), ("SOMA_LLM_MOCK", "fixed:ok")];
    let (out, err, code) = soma_in(&d, &["run", "--fresh", "app.cell", "ask", "hi"], &env);
    assert_eq!(code, 0, "{out}{err}");
    let _ = soma_in(&d, &["run", "app.cell", "boom"], &env);
    let text = std::fs::read_to_string(d.join("spans.jsonl")).unwrap();
    assert!(!text.contains("SECRET-PROMPT"), "content is opt-in: {text}");
    let spans: Vec<serde_json::Value> = text.lines().map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap()["resourceSpans"][0]["scopeSpans"][0]["spans"][0].clone()).collect();
    assert_eq!(spans.len(), 2);
    assert_eq!(spans[0]["name"], "Bot.ask");
    assert_eq!(spans[0]["traceId"].as_str().unwrap().len(), 32);
    assert_eq!(spans[0]["spanId"].as_str().unwrap().len(), 16);
    assert_eq!(spans[0]["status"]["code"], 1);
    let ev = &spans[0]["events"][0];
    assert_eq!(ev["name"], "gen_ai.chat");
    let keys: Vec<&str> = ev["attributes"].as_array().unwrap().iter().map(|a| a["key"].as_str().unwrap()).collect();
    assert!(keys.contains(&"gen_ai.usage.total_tokens") && keys.contains(&"gen_ai.prompt.chars") && !keys.contains(&"gen_ai.prompt"), "{keys:?}");
    assert_eq!(spans[1]["status"]["code"], 2);
    assert_eq!(spans[1]["status"]["message"], "not_found");
    // opt in to content
    let env = [("SOMA_OTEL_FILE", "with.jsonl"), ("SOMA_LLM_MOCK", "fixed:ok"), ("SOMA_OTEL_CONTENT", "1")];
    let _ = soma_in(&d, &["run", "app.cell", "ask", "hi"], &env);
    assert!(std::fs::read_to_string(d.join("with.jsonl")).unwrap().contains("SECRET-PROMPT hi"));
}
