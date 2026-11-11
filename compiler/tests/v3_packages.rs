// 3.0 — D3: the ecosystem floor. Eight packages in pure Soma, each with its
// proofs (`cell test`): money, time, auth, ratelimit, queue, text, vector,
// http_client. They pass check, test and verify on this toolchain, and work
// together from one application.

use std::process::Command;

const PACKAGES: [&str; 8] = ["money", "time", "auth", "ratelimit", "queue", "text", "vector", "http_client"];

fn repo() -> std::path::PathBuf { std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf() }

fn soma_in(dir: &std::path::Path, args: &[&str]) -> (String, i32) {
    let o = Command::new(env!("CARGO_BIN_EXE_soma")).args(args).current_dir(dir)
        .env_remove("SOMA_EDITION").output().expect("failed to run soma");
    (format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)), o.status.code().unwrap_or(-1))
}

fn copy_dir(from: &std::path::Path, to: &std::path::Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let name = e.file_name();
        if name.to_string_lossy().starts_with('.') { continue; }
        if e.path().is_dir() { copy_dir(&e.path(), &to.join(&name)); } else { std::fs::copy(e.path(), to.join(&name)).unwrap(); }
    }
}

/// A scratch project with the packages beside it (so no `.soma_data` is left in the repository).
fn project(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("soma_v3k_{}_{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for p in PACKAGES { copy_dir(&repo().join("packages").join(p), &dir.join("packages").join(p)); }
    dir
}

#[test]
fn d3_every_package_passes_its_own_proofs() {
    let dir = project("each");
    for p in PACKAGES {
        let file = format!("packages/{p}/{p}.cell");
        let (out, code) = soma_in(&dir, &["check", &file]);
        assert_eq!(code, 0, "{p}: {out}");
        assert!(!out.contains("warning:"), "{p} checks with a warning: {out}");
        let (out, code) = soma_in(&dir, &["test", &file]);
        assert_eq!(code, 0, "{p}: {out}");
        assert!(out.contains("0 failed"), "{p}: {out}");
        let (out, code) = soma_in(&dir, &["verify", &file]);
        assert_eq!(code, 0, "{p}: {out}");
        assert!(out.contains("VERIFY OK"), "{p}: {out}");
        // a package is published with what a consumer reads first
        for f in ["soma.toml", "README.md"] {
            assert!(repo().join("packages").join(p).join(f).exists(), "packages/{p}/{f} is missing");
        }
        let entry = std::fs::read_to_string(repo().join("site/repo").join(format!("{p}.json"))).unwrap_or_default();
        assert!(entry.contains(&format!("\"subdir\": \"packages/{p}\"")), "site/repo/{p}.json: {entry}");
    }
    let index = std::fs::read_to_string(repo().join("site/repo/index.json")).unwrap();
    for p in PACKAGES { assert!(index.contains(&format!("\"name\": \"{p}\"")), "site/repo/index.json does not list {p}"); }
}

#[test]
fn d3_the_packages_work_together_from_one_application() {
    let dir = project("together");
    std::fs::write(dir.join("app.cell"), r#"use money
use time
use auth
use ratelimit
use queue
use text
use vector
use http_client

cell Shop {
    face {
        signal checkout(user: String, cents: Int, now_s: Int) -> Map
    }

    on checkout(user: String, cents: Int, now_s: Int) {
        if !allow("checkout:" + user, 2, 60, now_s) { return response(429, map("retry_after", retry_after("checkout:" + user, 2, 60, now_s))) }
        let total = money(cents, "EUR")
        let parts = allocate(cents, [80, 20])
        let job = enqueue("receipts", map("user", user, "total", format_money(total)))
        let token = jwt_sign(map("sub", user, "exp", now_s + 600), "s3cret")
        return map("total", format_money(total), "net", parts[0], "tax", parts[1], "job", job,
            "at", format_iso(now_s, "Europe/Paris"), "sub", jwt_verify(token, "s3cret", now_s).sub,
            "slug", slug("Order for " + user), "near", cosine(embed_text("late parcel", 64), embed_text("parcel late", 64)) > 0.3,
            "url", with_query("https://x.test/r", map("u", user)))
    }
}

cell test Together {
    rules {
        let r = checkout("ann", 1999, 1774747800)
        assert r.total == "19.99 EUR"
        assert r.net + r.tax == 1999
        assert r.at == "2026-03-29T03:30:00+02:00"
        assert r.sub == "ann"
        assert r.slug == "order-for-ann"
        assert r.near
        assert r.url == "https://x.test/r?u=ann"
        assert claim("receipts", 1774747800, 30).payload.total == "19.99 EUR"
        assert checkout("ann", 100, 1774747801).total == "1.00 EUR"
        assert checkout("ann", 100, 1774747802)._status == 429
    }
}
"#).unwrap();
    let (out, code) = soma_in(&dir, &["check", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    let (out, code) = soma_in(&dir, &["test", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("10 passed, 0 failed"), "{out}");
    // the packages' state is the application's data: it is there on the next run
    let (out, code) = soma_in(&dir, &["run", "app.cell", "checkout", "bob", "500", "1774747800"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("5.00 EUR"), "{out}");
    let (_, code) = soma_in(&dir, &["run", "app.cell", "checkout", "bob", "500", "1774747801"]);
    assert_eq!(code, 0);
    let (out, _) = soma_in(&dir, &["run", "app.cell", "checkout", "bob", "500", "1774747802"]);
    assert!(out.contains("retry_after"), "the third checkout in the window is limited, across processes: {out}");
}
