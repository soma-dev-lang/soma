//! `soma example` — retrieve a verified program to start from.
//!
//! The corpus (300+ programs, each re-run through check/test/verify when the
//! site is built) is indexed at <site>/corpus/full.json. The fastest way to
//! a correct cell is a verified one that already does most of the job, so
//! this puts retrieval one command away:
//!
//!   soma example                         domains and features
//!   soma example invariant http          programs having both
//!   soma example escrow                  word search in id/title/summary
//!   soma example escrow_finance/<name>   print that program's source
//!
//! SOMA_SITE overrides the base URL (tests, mirrors, a local `site/`).

use serde_json::Value;

fn site() -> String {
    std::env::var("SOMA_SITE")
        .unwrap_or_else(|_| "https://soma-lang.dev".to_string())
        .trim_end_matches('/')
        .to_string()
}

fn fetch(url: &str) -> Result<String, String> {
    ureq::get(url)
        .timeout(std::time::Duration::from_secs(15))
        .call()
        .map_err(|e| format!("cannot fetch {}: {}", url, e))?
        .into_string()
        .map_err(|e| format!("cannot read {}: {}", url, e))
}

fn strs(v: &Value, key: &str) -> Vec<String> {
    v.get(key)
        .and_then(|x| x.as_array())
        .map(|a| a.iter().filter_map(|s| s.as_str().map(String::from)).collect())
        .unwrap_or_default()
}

pub fn cmd_example(terms: &[String], json: bool, all: bool) {
    let base = site();
    // full.json carries the summaries the word search needs
    let index_url = format!("{}/corpus/full.json", base);
    let index: Value = match fetch(&index_url).and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string())) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error: {}", e);
            eprintln!("  the corpus is also in the repository: examples/corpus/");
            std::process::exit(1);
        }
    };
    let programs = index.get("programs").and_then(|p| p.as_array()).cloned().unwrap_or_default();

    // no terms: the map of what exists
    if terms.is_empty() {
        if json {
            println!("{}", serde_json::json!({
                "count": index.get("count"),
                "domains": index.get("domains"),
                "features": index.get("features"),
            }));
            return;
        }
        println!("{} verified programs (soma {})\n", programs.len(),
            index.get("soma_version").and_then(|v| v.as_str()).unwrap_or("?"));
        println!("domains:  {}", index.get("domains").and_then(|d| d.as_object())
            .map(|d| d.iter().map(|(k, v)| format!("{}({})", k, v)).collect::<Vec<_>>().join(" "))
            .unwrap_or_default());
        println!("features: {}", strs(&index, "features").join(" "));
        println!("\nsoma example <domain|feature|word>...   list matches (terms are ANDed)");
        println!("soma example <id>                       print a program's source");
        return;
    }

    // exact id: print the source
    if terms.len() == 1 {
        if let Some(p) = programs.iter().find(|p| p.get("id").and_then(|i| i.as_str()) == Some(terms[0].as_str())) {
            // the index names the canonical URL; fetch relative to `base`
            // so SOMA_SITE mirrors work
            let url = format!("{}/corpus/{}.cell", base, terms[0]);
            match fetch(&url) {
                Ok(src) => {
                    if json {
                        let mut out = p.clone();
                        out["source"] = Value::String(src);
                        println!("{}", out);
                    } else {
                        print!("{}", src);
                    }
                }
                Err(e) => {
                    eprintln!("error: {}", e);
                    std::process::exit(1);
                }
            }
            return;
        }
    }

    // search: every term must hit the domain, a feature, or the text
    let wanted: Vec<String> = terms.iter().map(|t| t.to_lowercase()).collect();
    let hits: Vec<&Value> = programs
        .iter()
        .filter(|p| {
            let domain = p.get("domain").and_then(|d| d.as_str()).unwrap_or("").to_lowercase();
            let feats = strs(p, "features");
            let text = format!(
                "{} {} {}",
                p.get("id").and_then(|s| s.as_str()).unwrap_or(""),
                p.get("title").and_then(|s| s.as_str()).unwrap_or(""),
                p.get("summary").and_then(|s| s.as_str()).unwrap_or("")
            )
            .to_lowercase();
            wanted.iter().all(|t| domain == *t || feats.iter().any(|f| f == t) || text.contains(t.as_str()))
        })
        .collect();

    if json {
        println!("{}", serde_json::to_string(&hits).unwrap_or_else(|_| "[]".to_string()));
        return;
    }
    if hits.is_empty() {
        eprintln!("no verified program matches [{}]. `soma example` lists domains and features.", terms.join(", "));
        std::process::exit(1);
    }
    let shown = if all { hits.len() } else { 20 };
    for p in hits.iter().take(shown) {
        println!(
            "{:<44} {}",
            p.get("id").and_then(|s| s.as_str()).unwrap_or(""),
            p.get("title").and_then(|s| s.as_str()).unwrap_or("")
        );
        println!("{:<44} [{}]", "", strs(p, "features").join(", "));
    }
    if hits.len() > shown {
        // the terms that would narrow THIS list, not a vague "add a term"
        let mut domains: Vec<String> = hits.iter()
            .filter_map(|p| p.get("domain").and_then(|d| d.as_str()).map(|d| d.to_string()))
            .collect();
        domains.sort(); domains.dedup();
        let mut feats: Vec<String> = hits.iter().flat_map(|p| strs(p, "features")).collect();
        feats.sort(); feats.dedup();
        feats.retain(|f| !wanted.contains(f));
        println!("… {} more — `--all` lists every match, or narrow by domain [{}] or feature [{}]",
            hits.len() - shown, domains.join(", "), feats.join(", "));
    }
    println!("\nsoma example <id>    prints the source");
}
