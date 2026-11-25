//! `env_var` and `run_process`: what a program reads from its environment, and the programs
//! it starts. Both are effects a frame must name; `run_process` never goes through a shell and
//! never raises on the process it starts — a failure is `{error, kind}`.

use std::process::Command;

fn soma_env(dir: &std::path::Path, args: &[&str], env: &[(&str, &str)]) -> (String, i32) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_soma"));
    c.args(args).current_dir(dir).env_remove("SOMA_PROCESS").env_remove("SOMA_PROCESS_ALLOW");
    for (k, v) in env { c.env(k, v); }
    let o = c.output().expect("soma runs");
    (format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)), o.status.code().unwrap_or(-1))
}

fn project(name: &str, source: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("soma_process_{}", name));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("app.cell"), source).unwrap();
    dir
}

const PROG: &str = r#"cell P {
    on lire(nom: String) { return env_var(nom) ?? "absente" }
    on lancer(prog: String, args: List<String>) { return run_process(prog, args) }
    on avec(prog: String, args: List<String>, opts: Map) { return run_process(prog, args, opts) }
}
"#;

#[test]
fn env_var_reads_the_environment() {
    let dir = project("env", PROG);
    let (out, code) = soma_env(&dir, &["run", "app.cell", "lire", "ATTELAGE_ESSAI"], &[("ATTELAGE_ESSAI", "valeur-7")]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("valeur-7"), "{out}");
    let (out, _) = soma_env(&dir, &["run", "app.cell", "lire", "ATTELAGE_ABSENTE_XYZ"], &[]);
    assert!(out.contains("absente"), "{out}");
}

#[test]
fn run_process_starts_a_program_without_a_shell() {
    let dir = project("run", PROG);
    let (out, code) = soma_env(&dir, &["run", "app.cell", "lancer", "echo", "[\"bonjour\", \"$HOME\", \"a;b\"]"], &[]);
    assert_eq!(code, 0, "{out}");
    // the arguments are passed as they are: no expansion, no command separator
    assert!(out.contains("bonjour $HOME a;b"), "{out}");
    assert!(out.contains("code") && out.contains("0"), "{out}");
    // exit code and stderr
    let (out, _) = soma_env(&dir, &["run", "app.cell", "lancer", "sh", "[\"-c\", \"echo oups >&2; exit 3\"]"], &[]);
    assert!(out.contains("oups") && out.contains("3"), "{out}");
    // a program that does not exist: {error, kind: not_found}, no raise
    let (out, code) = soma_env(&dir, &["run", "app.cell", "lancer", "programme-qui-n-existe-pas-xyz", "[]"], &[]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("not_found"), "{out}");
}

#[test]
fn run_process_options_bound_what_it_does() {
    let dir = project("opts", PROG);
    // stdin, cwd, env
    let (out, _) = soma_env(&dir, &["run", "app.cell", "avec", "cat", "[]", "{\"stdin\": \"lu sur l'entrée\"}"], &[]);
    assert!(out.contains("lu sur l'entrée"), "{out}");
    let (out, _) = soma_env(&dir, &["run", "app.cell", "avec", "pwd", "[]", "{\"cwd\": \"/\"}"], &[]);
    assert!(out.contains("\"/\\n\"") || out.contains("/\n") || out.contains("stdout: /"), "{out}");
    let (out, _) = soma_env(&dir, &["run", "app.cell", "avec", "sh", "[\"-c\", \"echo $ATT_X\"]", "{\"env\": {\"ATT_X\": \"donnee\"}}"], &[]);
    assert!(out.contains("donnee"), "{out}");
    // the timeout kills it
    let (out, _) = soma_env(&dir, &["run", "app.cell", "avec", "sleep", "[\"5\"]", "{\"timeout\": 300}"], &[]);
    assert!(out.contains("timeout"), "{out}");
    // the output is cut at max_bytes
    let (out, _) = soma_env(&dir, &["run", "app.cell", "avec", "sh", "[\"-c\", \"yes | head -c 5000\"]", "{\"max_bytes\": 100}"], &[]);
    assert!(out.contains("truncated") && out.contains("true"), "{out}");
    // a wrong option value is a program bug: it raises
    let (out, code) = soma_env(&dir, &["run", "app.cell", "avec", "echo", "[]", "{\"timeout\": \"vite\"}"], &[]);
    assert!(code != 0 && out.contains("option 'timeout' must be"), "{out}");
}

#[test]
fn the_operator_can_refuse_processes() {
    let dir = project("policy", PROG);
    let (out, _) = soma_env(&dir, &["run", "app.cell", "lancer", "echo", "[\"x\"]"], &[("SOMA_PROCESS", "deny")]);
    assert!(out.contains("refused"), "{out}");
    let (out, _) = soma_env(&dir, &["run", "app.cell", "lancer", "echo", "[\"permis\"]"], &[("SOMA_PROCESS_ALLOW", "git,echo")]);
    assert!(out.contains("permis"), "{out}");
    let (out, _) = soma_env(&dir, &["run", "app.cell", "lancer", "ls", "[]"], &[("SOMA_PROCESS_ALLOW", "git,echo")]);
    assert!(out.contains("refused"), "{out}");
}

#[test]
fn both_are_effects_a_frame_names_and_a_condition_may_not_call() {
    let sk = "skeleton S {\n    face {\n        signal f(n: String) -> String\n        signal g(p: String) -> Map  CLAUSE\n    }\n}\nflesh S {\n    on f(n) { return env_var(n) ?? \"\" }\n    on g(p) { return run_process(p, []) }\n}\n";
    let dir = project("frame", &sk.replace("  CLAUSE", ""));
    let (out, code) = soma_env(&dir, &["check", "app.cell"], &[]);
    assert!(code != 0 && out.contains("calls env_var") && out.contains("calls run_process"), "{out}");
    let dir = project("frame_ok", &sk.replace("signal f(n: String) -> String", "signal f(n: String) -> String  calls env_var").replace("  CLAUSE", "  calls run_process"));
    let (out, code) = soma_env(&dir, &["check", "app.cell"], &[]);
    assert_eq!(code, 0, "{out}");
    // an invariant may not start a process
    let inv = "cell C {\n    memory {\n        x: Map<String, Int>\n        invariant len(run_process(\"echo\", [])) >= 0\n    }\n    on f() { x.set(\"a\", 1)  return 1 }\n}\n";
    let (out, code) = soma_env(&project("inv", inv), &["check", "app.cell"], &[]);
    assert!(code != 0 && out.contains("run_process"), "{out}");
    // [deterministic] refuses both
    let det = "cell D {\n    on f(n: String) [deterministic] { return env_var(n) ?? \"\" }\n}\n";
    let (out, code) = soma_env(&project("det", det), &["check", "app.cell"], &[]);
    assert!(code != 0 && out.contains("env_var"), "{out}");
}

#[test]
fn a_test_mocks_them_and_a_model_answer_reaching_one_is_flagged() {
    let t = "cell P {\n    on build() {\n        let r = run_process(\"cargo\", [\"build\"])\n        return r.code\n    }\n    on cle() { return env_var(\"CLE\") ?? \"\" }\n}\ncell test T {\n    rules {\n        mock run_process map(\"code\", 0, \"stdout\", \"ok\", \"stderr\", \"\", \"truncated\", false)\n        assert P.build() == 0\n        mock env_var \"secret\"\n        assert P.cle() == \"secret\"\n    }\n}\n";
    let (out, code) = soma_env(&project("mock", t), &["test", "app.cell"], &[]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("2 passed"), "{out}");
    // the program a model chose is a command it chose
    let taint = "cell A {\n    on f(q: String) {\n        let prog = think(q)\n        return run_process(prog, [])\n    }\n}\n";
    let (out, _) = soma_env(&project("taint", taint), &["check", "app.cell"], &[]);
    assert!(out.contains("run_process") && out.contains("program"), "{out}");
}
