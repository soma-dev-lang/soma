//! `soma verify --attest out.json` (3.0): the proof as an artifact.
//!
//! The attestation names what was verified (a sha256 of every source file
//! and of the soma.toml beside it), by what (toolchain version and a sha256
//! of the binary), and each property with its verdict CLASS — `proven`,
//! `runtime-checked` (enforced on every write, not proven), `unproven`,
//! `failed`, `note`. Its bytes are a function of the sources and the
//! toolchain: a reviewer runs `soma verify --check-attestation out.json`
//! and compares digests instead of reading a pasted terminal.

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().iter().map(|b| format!("{:02x}", b)).collect()
}

fn file_digest(path: &Path) -> Result<String, String> {
    std::fs::read(path).map(|b| sha256_hex(&b)).map_err(|e| format!("cannot read '{}': {}", path.display(), e))
}

/// `use` targets of a program that are files of the project: beside the
/// importing file, or from the entry's directory (`use side` is `side.cell`
/// when that file is there — a package otherwise, pinned by soma.lock).
fn local_imports(file: &Path, seen: &mut Vec<PathBuf>) {
    let root = file.parent().map(|d| d.to_path_buf()).unwrap_or_default();
    fn rec(file: &Path, root: &Path, seen: &mut Vec<PathBuf>) {
        let Ok(source) = std::fs::read_to_string(file) else { return };
        let Ok(tokens) = crate::lexer::Lexer::new(&source).tokenize() else { return };
        let Ok(program) = crate::parser::Parser::new(tokens).parse_program() else { return };
        let dir = file.parent().map(|d| d.to_path_buf()).unwrap_or_default();
        for import in &program.imports {
            if import.starts_with("std:") { continue; }
            let rel = match (import.strip_prefix("lib:"), import.strip_prefix("pkg:")) {
                (Some(r), _) => format!("lib/{r}"),
                (_, Some(r)) => r.to_string(),
                _ => import.clone(),
            };
            let rel = if rel.ends_with(".cell") { rel } else { format!("{rel}.cell") };
            for candidate in [dir.join(&rel), root.join(&rel)] {
                if candidate.exists() {
                    if !seen.contains(&candidate) {
                        seen.push(candidate.clone());
                        rec(&candidate, root, seen);
                    }
                    break;
                }
            }
        }
    }
    rec(file, &root, seen);
}

fn class_of(status: &str, message: &str) -> &'static str {
    match status {
        "pass" => "proven",
        "fail" | "error" => "failed",
        "note" => "note",
        _ if message.contains("runtime-checked") => "runtime-checked",
        _ => "unproven",
    }
}

/// Run `soma verify --json` on the files (a fresh process: its report is
/// exactly what a reviewer's run prints) and build the attestation.
pub fn build(files: &[PathBuf], strict: bool) -> Result<serde_json::Value, String> {
    let exe = std::env::current_exe().map_err(|e| format!("cannot locate the soma binary: {e}"))?;
    let mut cmd = std::process::Command::new(&exe);
    cmd.arg("verify").arg("--json");
    if strict { cmd.arg("--strict"); }
    cmd.args(files);
    let out = cmd.output().map_err(|e| format!("cannot run soma verify: {e}"))?;
    let report: serde_json::Value = serde_json::from_slice(&out.stdout)
        .map_err(|e| format!("soma verify --json did not print one JSON document ({e})"))?;

    let mut sources: Vec<PathBuf> = Vec::new();
    for f in files {
        if !sources.contains(f) { sources.push(f.clone()); }
        local_imports(f, &mut sources);
        for beside in ["soma.toml", "soma.lock"] {
            let p = f.parent().map(|d| d.join(beside)).unwrap_or_else(|| PathBuf::from(beside));
            if p.exists() && !sources.contains(&p) { sources.push(p); }
        }
    }
    let mut source_entries = Vec::new();
    for s in &sources {
        source_entries.push(serde_json::json!({"file": s.display().to_string(), "sha256": file_digest(s)?}));
    }

    let mut properties = Vec::new();
    let mut tally = std::collections::BTreeMap::new();
    let mut add = |scope: String, status: &str, message: &str, properties: &mut Vec<serde_json::Value>| {
        let class = class_of(status, message);
        *tally.entry(class).or_insert(0usize) += 1;
        properties.push(serde_json::json!({"scope": scope, "class": class, "statement": message}));
    };
    for sm in report["state_machines"].as_array().into_iter().flatten() {
        let scope = sm["name"].as_str().unwrap_or("").to_string();
        for c in sm["checks"].as_array().into_iter().flatten() {
            add(scope.clone(), c["status"].as_str().unwrap_or(""), c["message"].as_str().unwrap_or(""), &mut properties);
        }
    }
    for t in report["temporal"].as_array().into_iter().flatten() {
        let scope = t["state_machine"].as_str().unwrap_or("").to_string();
        for p in t["properties"].as_array().into_iter().flatten() {
            let status = if p["passed"].as_bool().unwrap_or(false) { "pass" } else { "fail" };
            let statement = format!("{}: {}", p["property"].as_str().unwrap_or(""), p["message"].as_str().unwrap_or(""));
            add(scope.clone(), status, &statement, &mut properties);
        }
    }
    for h in report["hordes"].as_array().into_iter().flatten() {
        add("horde".to_string(), h["status"].as_str().unwrap_or(""), h["message"].as_str().unwrap_or(""), &mut properties);
    }
    for e in report["errors"].as_array().into_iter().flatten().chain(report["check_errors"].as_array().into_iter().flatten()) {
        let msg = e["message"].as_str().or(e.as_str()).unwrap_or("");
        add("program".to_string(), "fail", msg, &mut properties);
    }

    let manifest = files.first().and_then(|f| f.parent()).map(|d| d.join("soma.toml"))
        .filter(|p| p.exists())
        .and_then(|p| crate::pkg::manifest::Manifest::load(&p).ok());
    let mut doc = serde_json::json!({
        "soma_attestation": 1,
        "toolchain": {
            "version": env!("CARGO_PKG_VERSION"),
            "binary_sha256": file_digest(&exe)?,
        },
        "edition": crate::pkg::manifest::Manifest::edition_of(manifest.as_ref()),
        "strict": strict,
        "sources": source_entries,
        "ok": report["ok"],
        "verdict": report["verdict"],
        "summary": tally,
        "properties": properties,
        "scope": "per cell: state machines, temporal properties, memory invariants, cost bounds. `proven` holds for every execution; `runtime-checked` is enforced before each write and not proven; nothing here covers composition between cells",
    });
    // the digest covers everything above, in this key order
    let digest = sha256_hex(serde_json::to_string(&doc).unwrap().as_bytes());
    doc["digest"] = serde_json::json!(digest);
    Ok(doc)
}

pub fn write(files: &[PathBuf], strict: bool, out: &Path) -> ! {
    match build(files, strict) {
        Ok(doc) => {
            let text = serde_json::to_string_pretty(&doc).unwrap() + "\n";
            if let Err(e) = std::fs::write(out, text) {
                eprintln!("error: cannot write '{}': {}", out.display(), e);
                std::process::exit(1);
            }
            let s = &doc["summary"];
            println!("{}", doc["verdict"].as_str().unwrap_or(""));
            println!("attested {} file(s) → {}", doc["sources"].as_array().map_or(0, |a| a.len()), out.display());
            println!("  proven: {}  runtime-checked: {}  unproven: {}  failed: {}",
                s["proven"].as_u64().unwrap_or(0), s["runtime-checked"].as_u64().unwrap_or(0),
                s["unproven"].as_u64().unwrap_or(0), s["failed"].as_u64().unwrap_or(0));
            println!("  digest: {}", doc["digest"].as_str().unwrap_or(""));
            std::process::exit(if doc["ok"].as_bool().unwrap_or(false) { 0 } else { 1 });
        }
        Err(e) => { eprintln!("error: {e}"); std::process::exit(1); }
    }
}

/// Re-run the verification an attestation records and compare: the same
/// sources and toolchain must give the same digest.
pub fn check(attestation: &Path) -> ! {
    let fail = |msg: String| -> ! { eprintln!("ATTESTATION REJECTED — {msg}"); std::process::exit(1) };
    let text = std::fs::read_to_string(attestation).unwrap_or_else(|e| fail(format!("cannot read '{}': {}", attestation.display(), e)));
    let doc: serde_json::Value = serde_json::from_str(&text).unwrap_or_else(|e| fail(format!("not JSON: {e}")));
    if doc["soma_attestation"] != 1 { fail("not a soma attestation (no `soma_attestation: 1`)".to_string()); }
    // the document is intact: its digest is the digest of its own content
    let mut body = doc.clone();
    let claimed = body.as_object_mut().and_then(|o| o.remove("digest")).and_then(|d| d.as_str().map(|s| s.to_string())).unwrap_or_default();
    if sha256_hex(serde_json::to_string(&body).unwrap().as_bytes()) != claimed {
        fail("the document was edited after it was written (its digest does not match its content)".to_string());
    }
    for s in doc["sources"].as_array().into_iter().flatten() {
        let file = s["file"].as_str().unwrap_or("");
        match file_digest(Path::new(file)) {
            Ok(d) if d == s["sha256"].as_str().unwrap_or("") => {}
            Ok(_) => fail(format!("{file} changed since it was attested")),
            Err(e) => fail(e),
        }
    }
    let files: Vec<PathBuf> = doc["sources"].as_array().into_iter().flatten()
        .filter_map(|s| s["file"].as_str())
        .filter(|f| f.ends_with(".cell"))
        .map(PathBuf::from).collect();
    // the files given to verify are the ones no other attested file imports
    let mut imported = Vec::new();
    for f in &files { local_imports(f, &mut imported); }
    let roots: Vec<PathBuf> = files.iter().filter(|f| !imported.contains(f)).cloned().collect();
    let fresh = build(&roots, doc["strict"].as_bool().unwrap_or(false)).unwrap_or_else(|e| fail(e));
    if fresh["toolchain"] != doc["toolchain"] {
        eprintln!("note: attested with soma {} (binary {}…), checked with soma {} (binary {}…)",
            doc["toolchain"]["version"].as_str().unwrap_or("?"), &doc["toolchain"]["binary_sha256"].as_str().unwrap_or("").chars().take(12).collect::<String>(),
            fresh["toolchain"]["version"].as_str().unwrap_or("?"), &fresh["toolchain"]["binary_sha256"].as_str().unwrap_or("").chars().take(12).collect::<String>());
        // a different toolchain may still reach the same verdicts: compare those
        if fresh["properties"] != doc["properties"] || fresh["verdict"] != doc["verdict"] {
            fail("this toolchain does not reach the attested verdicts".to_string());
        }
        println!("ATTESTATION CONFIRMED (different toolchain, same verdicts) — {}", doc["verdict"].as_str().unwrap_or(""));
        std::process::exit(if doc["ok"].as_bool().unwrap_or(false) { 0 } else { 1 });
    }
    if fresh["digest"] != doc["digest"] {
        fail("re-running the verification gives a different result".to_string());
    }
    println!("ATTESTATION CONFIRMED — {} (digest {})", doc["verdict"].as_str().unwrap_or(""), claimed);
    std::process::exit(if doc["ok"].as_bool().unwrap_or(false) { 0 } else { 1 });
}
