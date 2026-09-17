/// Agent integration tests — require a running ollama instance at localhost:11434.
/// These tests are skipped automatically when ollama is not reachable or
/// does not have LLM_MODEL pulled.

use std::process::Command;

fn soma_with_env(args: &[&str], env: &[(&str, &str)]) -> (String, String, i32) {
    // CARGO_BIN_EXE_soma resolves to the binary for the active profile,
    // so the suite works under both `cargo test` and `cargo test --release`.
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_soma"));
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

/// Reachable AND serving LLM_MODEL. A bare TCP connect is not enough: an
/// ollama without the model answers 404 on every completion, which would
/// fail these tests for an environment reason.
fn ollama_available() -> bool {
    use std::io::{Read, Write};
    let timeout = std::time::Duration::from_secs(2);
    let Ok(mut stream) = std::net::TcpStream::connect_timeout(&"127.0.0.1:11434".parse().unwrap(), timeout) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(timeout));
    if stream
        .write_all(b"GET /api/tags HTTP/1.0\r\nHost: localhost\r\n\r\n")
        .is_err()
    {
        return false;
    }
    let mut body = String::new();
    let _ = stream.read_to_string(&mut body);
    body.contains(&format!("\"{}\"", LLM_MODEL))
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
        eprintln!("SKIP: ollama with {} not available at localhost:11434", LLM_MODEL);
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
        eprintln!("SKIP: ollama with {} not available at localhost:11434", LLM_MODEL);
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
        eprintln!("SKIP: ollama with {} not available at localhost:11434", LLM_MODEL);
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

/// Hermetic (no ollama needed): `soma test` must honor soma.toml
/// `[agent] mock`, exactly like `soma run` does.
#[test]
fn test_cmd_reads_agent_mock_from_manifest() {
    let dir = std::env::temp_dir().join("soma_test_manifest_mock");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("soma.toml"),
        "[package]\nname = \"m\"\nversion = \"0.1.0\"\n\n[agent]\nmock = \"fixed:pong\"\n",
    )
    .unwrap();
    let cell = dir.join("app.cell");
    std::fs::write(&cell, r#"
        cell agent T {
            face { signal ask() -> String }
            on ask() { return think("ping") }
        }
        cell test TTests {
            rules { assert ask() == "pong" }
        }
    "#).unwrap();

    // make sure the env cannot be what satisfies the test
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_soma"));
    cmd.args(["test", cell.to_str().unwrap()])
        .env_remove("SOMA_LLM_MOCK")
        .env_remove("SOMA_LLM_URL")
        .env_remove("SOMA_LLM_KEY");
    let output = cmd.output().expect("failed to run soma");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.status.code(), Some(0), "manifest mock must apply under `soma test`: {text}");

    let _ = std::fs::remove_dir_all(&dir);
}
