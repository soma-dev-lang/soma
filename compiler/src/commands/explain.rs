use crate::checker::codes;

/// `soma explain E0028`: what a diagnostic code means. Codes are stable —
/// a code never changes meaning between releases.
pub fn cmd_explain(code: &str, json: bool) {
    if code.is_empty() {
        let all = codes::all();
        if json {
            let v: Vec<serde_json::Value> = all.iter()
                .map(|(c, k, d)| serde_json::json!({"code": c, "kind": k, "meaning": d}))
                .collect();
            println!("{}", serde_json::to_string_pretty(&v).unwrap());
        } else {
            for (c, k, d) in all {
                println!("{c}  {k:<28} {d}");
            }
        }
        return;
    }
    match codes::explain(code) {
        Some((kind, meaning)) => {
            let code = code.to_uppercase();
            if json {
                println!("{}", serde_json::json!({"code": code, "kind": kind, "meaning": meaning}));
            } else {
                println!("{code} ({kind}): {meaning}");
                println!("  the diagnostic's own message names the fix; `soma check --json` gives it as `fix`");
                println!("  verified wrong -> right pairs: soma docs gotchas");
            }
        }
        None => {
            eprintln!("error: no diagnostic code '{code}' — `soma explain` lists them");
            std::process::exit(1);
        }
    }
}
