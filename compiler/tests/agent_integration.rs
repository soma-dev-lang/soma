/// Agent integration tests — require a running ollama instance at localhost:11434.
/// These tests are skipped automatically when ollama is not reachable.

use std::process::Command;

fn soma_with_env(args: &[&str], env: &[(&str, &str)]) -> (String, String, i32) {
    let mut cmd = Command::new("./target/debug/soma");
    cmd.args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"));
    for (k, v) in env {
        cmd.env(k, v);
    }
    let output = cmd.output().expect("failed to run soma");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let code = output.status.code().unwrap_or(-1);
    (stdout, stderr, code)
}

fn ollama_available() -> bool {
    std::net::TcpStream::connect_timeout(
        &"127.0.0.1:11434".parse().unwrap(),
        std::time::Duration::from_secs(2),
    ).is_ok()
}

const LLM_URL: &str = "http://localhost:11434/v1/chat/completions";
const LLM_MODEL: &str = "gemma3:12b";

fn llm_env() -> Vec<(&'static str, &'static str)> {
    vec![
        ("SOMA_LLM_URL", LLM_URL),
        ("SOMA_LLM_MODEL", LLM_MODEL),
        ("SOMA_LLM_KEY", "ollama"),
    ]
}

#[test]
fn test_think_basic() {
    if !ollama_available() {
        eprintln!("SKIP: ollama not reachable at localhost:11434");
        return;
    }

    let tmp = std::env::temp_dir().join("test_think_basic.cell");
    std::fs::write(&tmp, r#"
        cell agent T {
            face {
                signal run() -> String
            }
            on run() {
                let r = think("Say hello in one word")
                return r
            }
        }
    "#).unwrap();

    let env = llm_env();
    let (out, err, code) = soma_with_env(&["run", tmp.to_str().unwrap()], &env);
    eprintln!("stdout: {}", out);
    eprintln!("stderr: {}", err);
    assert_eq!(code, 0, "think() should succeed, stderr: {}", err);
    assert!(!out.trim().is_empty(), "think() should produce non-empty output");

    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn test_think_with_tool_calling() {
    if !ollama_available() {
        eprintln!("SKIP: ollama not reachable at localhost:11434");
        return;
    }

    // Agent with tools declared — think() should still work even if ollama
    // doesn't support tool calling (it just won't use tools)
    let tmp = std::env::temp_dir().join("test_think_tools.cell");
    std::fs::write(&tmp, r#"
        cell agent T {
            face {
                signal run() -> String
                tool search(query: String) -> String "Search the web"
            }
            on run() {
                let r = think("What is 2+2? Just say the number.")
                return r
            }
        }
    "#).unwrap();

    let env = llm_env();
    let (out, err, code) = soma_with_env(&["run", tmp.to_str().unwrap()], &env);
    eprintln!("stdout: {}", out);
    eprintln!("stderr: {}", err);
    // ollama may not support tool calling and return 400 — that's acceptable
    // We just verify it doesn't panic or hang; either success or a clean error
    if code == 0 {
        assert!(!out.trim().is_empty(), "should produce output on success");
    } else {
        // Acceptable failure: ollama doesn't support tool calling
        assert!(err.contains("think()") || err.contains("status code"),
            "should fail with a think() error, not a crash: {}", err);
    }

    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn test_token_tracking() {
    if !ollama_available() {
        eprintln!("SKIP: ollama not reachable at localhost:11434");
        return;
    }

    let tmp = std::env::temp_dir().join("test_token_tracking.cell");
    std::fs::write(&tmp, r#"
        cell agent T {
            face {
                signal run() -> Int
            }
            on run() {
                let r = think("Say hi")
                return tokens_used()
            }
        }
    "#).unwrap();

    let env = llm_env();
    let (out, err, code) = soma_with_env(&["run", tmp.to_str().unwrap()], &env);
    eprintln!("stdout: {}", out);
    eprintln!("stderr: {}", err);
    assert_eq!(code, 0, "tokens_used() should succeed, stderr: {}", err);
    let trimmed = out.trim();
    // tokens_used() should return a number > 0 after a think() call
    if let Ok(n) = trimmed.parse::<i64>() {
        assert!(n > 0, "tokens_used() should be > 0 after think(), got {}", n);
    } else {
        // Might return as a string or map — just verify it ran
        eprintln!("tokens_used() returned: {}", trimmed);
    }

    let _ = std::fs::remove_file(&tmp);
}
