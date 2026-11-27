//! The unit of an agent's work is the hole: `soma next` says the one thing to
//! do now, `soma fill` writes the body of ONE signal and can touch nothing
//! else. (Design note "l'outillage qui rend un agent efficace", lot B, as
//! commands — the repository distributes no MCP server.)

use std::io::Write;
use std::process::{Command, Stdio};

fn soma(dir: &std::path::Path, args: &[&str], stdin: &str) -> (String, String, i32) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_soma"));
    c.args(args).current_dir(dir).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = c.spawn().expect("soma runs");
    child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    let o = child.wait_with_output().unwrap();
    (String::from_utf8_lossy(&o.stdout).to_string(), String::from_utf8_lossy(&o.stderr).to_string(), o.status.code().unwrap_or(-1))
}

fn starter(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("soma_agent_fill_{}", name));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let (out, err, code) = soma(&dir, &["init", "--skeleton", "demo"], "");
    assert_eq!(code, 0, "{out}{err}");
    dir.join("demo")
}

fn next(p: &std::path::Path, more: &[&str]) -> serde_json::Value {
    let mut args = vec!["next", "app.cell", "--json"];
    args.extend_from_slice(more);
    let (out, err, _) = soma(p, &args, "");
    serde_json::from_str(&out).unwrap_or_else(|_| panic!("soma next --json: {out}{err}"))
}

/// Acceptance 7: on the starter, the program is finished by calling only
/// `next`, the brief, `fill` and `check`, and `next` ends on "complete".
#[test]
fn the_starter_is_finished_with_next_brief_and_fill() {
    let p = starter("loop");
    let n = next(&p, &[]);
    assert_eq!(n["kind"], "hole", "{n}");
    assert_eq!(n["target"], "Counter.reset");
    assert_eq!(n["command"], "soma skeleton app.cell Counter.reset");
    assert!(n["then"].as_str().unwrap().contains("soma fill app.cell Counter.reset"), "{n}");
    let (brief, _, code) = soma(&p, &["skeleton", "app.cell", "Counter.reset"], "");
    assert_eq!(code, 0);
    assert!(brief.contains("writes") && brief.contains("counts: Map<String, Int>"), "{brief}");
    let app = std::fs::read_to_string(p.join("app.cell")).unwrap();
    let (out, err, code) = soma(&p, &["fill", "app.cell", "Counter.reset"], "counts.set(\"n\", 0)\nreturn 0\n");
    assert_eq!(code, 0, "{out}{err}");
    assert!(out.contains("Counter.reset") && out.contains("0 holes left"), "{out}");
    assert_eq!(std::fs::read_to_string(p.join("app.cell")).unwrap(), app, "the skeleton is as it was");
    let flesh = std::fs::read_to_string(p.join("lib/counter.cell")).unwrap();
    assert!(flesh.contains("    on reset() {\n        counts.set(\"n\", 0)\n        return 0\n    }\n"), "{flesh}");
    let (_, _, code) = soma(&p, &["check", "app.cell"], "");
    assert_eq!(code, 0);
    let n = next(&p, &[]);
    assert_eq!(n["kind"], "complete", "{n}");
    assert!(n["reason"].as_str().unwrap().contains("VERIFY OK"), "{n}");
    let (out, _, code) = soma(&p, &["next", "app.cell"], "");
    assert_eq!(code, 0);
    assert!(out.starts_with("complete"), "{out}");
}

/// Acceptance 8: a body outside its frame comes back with the frame error
/// that exists, and only the diagnostics of that signal.
#[test]
fn fill_reports_the_frame_and_only_this_signal() {
    let p = starter("frame");
    // another handler is made wrong first: not this fill's business
    let path = p.join("lib/counter.cell");
    let before = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, before.replace("on total() { return counts.get(\"n\") ?? 0 }", "on total() { return nowhere_q }")).unwrap();
    let (out, err, code) = soma(&p, &["fill", "app.cell", "Counter.reset", "--json"], "counts.set(\"n\", 0)\nlet s = open_session(\"a\")\nreturn 0\n");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap_or_else(|_| panic!("{out}{err}"));
    assert_ne!(code, 0);
    assert_eq!(v["written"], true, "{out}");
    assert_eq!(v["frame"], "broken");
    let ds = v["diagnostics"].as_array().unwrap();
    assert_eq!(ds.len(), 1, "{out}");
    assert_eq!(ds[0]["code"], "E0120");
    assert!(ds[0]["message"].as_str().unwrap().contains("outside its frame"), "{out}");
    assert_eq!(v["elsewhere"], 1, "the other handler's error is counted, not shown: {out}");
    assert_eq!(v["holes_left"], 0);
    // the same body, rejected rather than left on disk
    std::fs::write(&path, &before).unwrap();
    let (out, err, code) = soma(&p, &["fill", "app.cell", "Counter.reset", "--reject-on-error"], "let s = open_session(\"a\")\nreturn 0\n");
    assert_ne!(code, 0, "{out}{err}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), before, "nothing is left of a rejected body");
    assert!(format!("{out}{err}").contains("E0120"), "{out}{err}");
    let n = next(&p, &[]);
    assert_eq!(n["kind"], "hole", "the hole is still one: {n}");
}

/// Acceptance 9: `fill` cannot change a skeleton, another handler or a law.
#[test]
fn fill_writes_one_handler_and_nothing_else() {
    let p = starter("only");
    std::fs::create_dir_all(p.join("lois")).unwrap();
    std::fs::write(p.join("lois/x.law"), "law X\n\nterm Compte \"Un compte.\"\n").unwrap();
    let files = ["app.cell", "lib/counter.cell", "lois/x.law", "soma.toml"];
    let read = |f: &str| std::fs::read_to_string(p.join(f)).unwrap();
    let before: Vec<String> = files.iter().map(|f| read(f)).collect();
    let unchanged = |why: &str| for (f, b) in files.iter().zip(&before) { assert_eq!(&read(f), b, "{why}: {f} changed"); };
    // a body that closes its handler to rewrite another one, a skeleton or to open a cell
    for body in [
        "return 0\n    }\n    on total() { return 99",
        "return 0 }\n}\nskeleton Counter { face { signal reset() -> Int } }\nflesh Counter {\n on reset() { return 1",
        "return 0 } }",
        "return {",
    ] {
        let (out, err, code) = soma(&p, &["fill", "app.cell", "Counter.reset"], body);
        assert_ne!(code, 0, "{body}: {out}{err}");
        assert!(err.contains("nothing was written"), "{body}: {err}");
        unchanged(body);
    }
    // a signal the face does not declare
    let (_, err, code) = soma(&p, &["fill", "app.cell", "Counter.vider"], "return 0");
    assert_ne!(code, 0);
    assert!(err.contains("vider") && err.contains("face") && err.contains("--amend"), "{err}");
    unchanged("an undeclared signal");
    // a cell that is not a skeleton, a cell that does not exist, a law
    let (_, err, code) = soma(&p, &["fill", "app.cell", "CounterTests.x"], "return 0");
    assert_ne!(code, 0);
    assert!(err.contains("not a skeleton"), "{err}");
    let (_, err, code) = soma(&p, &["fill", "app.cell", "Nulle.part"], "return 0");
    assert_ne!(code, 0);
    assert!(err.contains("Nulle"), "{err}");
    let (_, err, code) = soma(&p, &["fill", "lois/x.law", "Counter.reset"], "return 0");
    assert_ne!(code, 0, "{err}");
    unchanged("refused targets");
    // a real fill changes the flesh file inside one handler only
    let (out, err, code) = soma(&p, &["fill", "app.cell", "Counter.reset"], "counts.set(\"n\", 0)\nreturn 0");
    assert_eq!(code, 0, "{out}{err}");
    let after = read("lib/counter.cell");
    let added = "    on reset() {\n        counts.set(\"n\", 0)\n        return 0\n    }\n";
    assert_eq!(after.replacen(added, "", 1).replace("\n\n}", "\n}"), before[1].replace("\n\n}", "\n}"), "only the handler was added");
    for f in ["app.cell", "lois/x.law", "soma.toml"] { assert_eq!(read(f), before[files.iter().position(|x| *x == f).unwrap()]); }
    // filling it again replaces that handler, and only it
    let (out, err, code) = soma(&p, &["fill", "app.cell", "Counter.reset"], "counts.set(\"n\", 0)\nreturn total()");
    assert_ne!(code, 0, "`total` is outside reset's frame: {out}{err}");
    let again = read("lib/counter.cell");
    assert_eq!(again.matches("on reset(").count(), 1, "{again}");
    assert_eq!(again, after.replace("        return 0\n    }\n", "        return total()\n    }\n"));
}

/// A private helper is written by the same command, whole.
#[test]
fn fill_writes_a_private_helper_of_the_cell() {
    let p = starter("helper");
    let (out, err, code) = soma(&p, &["fill", "app.cell", "Counter._zero"], "on _zero(k: String) {\n    counts.set(k, 0)\n    return 0\n}\n");
    assert_eq!(code, 0, "{out}{err}");
    let (out, err, code) = soma(&p, &["fill", "app.cell", "Counter.reset"], "return _zero(\"n\")");
    assert_eq!(code, 0, "{out}{err}");
    let flesh = std::fs::read_to_string(p.join("lib/counter.cell")).unwrap();
    assert!(flesh.contains("    on _zero(k: String) {\n        counts.set(k, 0)\n        return 0\n    }\n"), "{flesh}");
    let (out, _, code) = soma(&p, &["test", "app.cell"], "");
    assert_eq!(code, 0, "{out}");
    // a helper needs its parameters: a bare body is refused
    let (_, err, code) = soma(&p, &["fill", "app.cell", "Counter._autre"], "return 0");
    assert_ne!(code, 0);
    assert!(err.contains("on _autre("), "{err}");
}

/// `next` hands out one thing, from an ordered queue.
#[test]
fn next_follows_its_queue() {
    let p = starter("queue");
    let flesh = p.join("lib/counter.cell");
    let good = std::fs::read_to_string(&flesh).unwrap();
    // 2. an error of check comes before the hole
    std::fs::write(&flesh, good.replace("on total() { return counts.get(\"n\") ?? 0 }", "on total() { return nowhere_q }")).unwrap();
    let n = next(&p, &[]);
    assert_eq!(n["kind"], "check", "{n}");
    assert!(n["target"].as_str().unwrap().starts_with("lib/counter.cell:"), "{n}");
    assert!(n["reason"].as_str().unwrap().contains("nowhere_q"), "{n}");
    let (_, _, code) = soma(&p, &["next", "app.cell"], "");
    assert_ne!(code, 0, "something is left to do");
    std::fs::write(&flesh, &good).unwrap();
    // 1. a gap `doctor` reports comes before everything
    std::fs::remove_file(p.join(".claude/settings.json")).unwrap();
    let n = next(&p, &[]);
    assert_eq!(n["kind"], "doctor", "{n}");
    assert_eq!(n["command"], "soma init --agent");
    let _ = soma(&p, &["init", "--agent"], "");
    // 5. the hole; a filter on a cell that has none skips to what follows
    assert_eq!(next(&p, &[])["kind"], "hole");
    let n = next(&p, &["--cell", "CounterTests"]);
    assert_ne!(n["kind"], "hole", "{n}");
    // 6. a failing test, once no hole is left
    let (_, _, code) = soma(&p, &["fill", "app.cell", "Counter.reset"], "counts.set(\"n\", 7)\nreturn 7");
    assert_eq!(code, 0);
    let app = std::fs::read_to_string(p.join("app.cell")).unwrap();
    let at = app.rfind("assert ").unwrap();
    std::fs::write(p.join("app.cell"), format!("{}assert reset() == 0\n        {}", &app[..at], &app[at..])).unwrap();
    let (_, _, c) = soma(&p, &["skeleton", "app.cell", "--amend"], "");
    let _ = c;
    let n = next(&p, &[]);
    assert_eq!(n["kind"], "test", "{n}");
    assert!(n["reason"].as_str().unwrap().contains("reset() == 0"), "{n}");
    // 7. complete
    let (_, _, code) = soma(&p, &["fill", "app.cell", "Counter.reset"], "counts.set(\"n\", 0)\nreturn 0");
    assert_eq!(code, 0);
    assert_eq!(next(&p, &[])["kind"], "complete");
}
