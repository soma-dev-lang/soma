//! What a program holds in memory while it runs.

use std::process::Command;

fn dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("soma_memory_{}_{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Peak resident memory of `soma <args>` in megabytes, as `/usr/bin/time -l` reports it.
#[cfg(target_os = "macos")]
fn peak_mb(d: &std::path::Path, args: &[&str]) -> (u64, String) {
    let o = Command::new("/usr/bin/time").arg("-l").arg(env!("CARGO_BIN_EXE_soma")).args(args).current_dir(d).output().expect("failed to run soma");
    let err = String::from_utf8_lossy(&o.stderr).to_string();
    let bytes: u64 = err.lines().find(|l| l.contains("maximum resident set size"))
        .and_then(|l| l.split_whitespace().next()).and_then(|n| n.parse().ok()).unwrap_or_else(|| panic!("no peak in: {err}"));
    (bytes / (1024 * 1024), format!("{}{}", String::from_utf8_lossy(&o.stdout), err))
}

/// A list built by `push` and dropped, forty times: the program holds one
/// list at a time, and its memory stayed that of forty. Soma freed each
/// list; macOS's allocator kept every freed block of a list grown by
/// `realloc` (a `Vec` that doubles) and reused none — 115 MB more per turn,
/// 4.5 GB on a strategy's history. Seen from outside as "memory that is
/// never released". Growing a large block is now allocate, copy, free.
#[cfg(target_os = "macos")]
#[test]
fn a_list_built_and_dropped_many_times_costs_one_list() {
    let d = dir("lists");
    std::fs::write(d.join("app.cell"), r#"
cell R {
    on une(n: Int) {
        let xs = []
        for i in range(0, n) { xs.push(to_float(i) * 0.5) }
        return len(xs)
    }
    on tours(n: Int, fois: Int) {
        let total = 0
        for k in range(0, fois) { total = total + une(n) }
        return total
    }
    on sur_place(n: Int, fois: Int) {
        let total = 0
        for k in range(0, fois) {
            let xs = []
            for i in range(0, n) { xs.push(to_float(i) * 0.5) }
            let ys = xs.map(x => x * 2.0)
            total = total + len(ys)
        }
        return total
    }
}
"#).unwrap();
    let (one, out) = peak_mb(&d, &["run", "app.cell", "tours", "300000", "1"]);
    assert!(out.contains("300000"), "{out}");
    let (forty, out) = peak_mb(&d, &["run", "app.cell", "tours", "300000", "40"]);
    assert!(out.contains("12000000"), "{out}");
    assert!(forty < one * 3, "forty lists built and dropped held {forty} MB, one list {one} MB");
    let (one, _) = peak_mb(&d, &["run", "app.cell", "sur_place", "300000", "1"]);
    let (forty, _) = peak_mb(&d, &["run", "app.cell", "sur_place", "300000", "40"]);
    assert!(forty < one * 3, "forty turns in one handler held {forty} MB, one turn {one} MB");
}

/// The same in a `[native]` handler, which is a library with an allocator of
/// its own: a `strbuf` grown by `sb_push`, forty calls.
#[cfg(target_os = "macos")]
#[test]
fn a_native_string_built_many_times_costs_one_string() {
    let d = dir("native");
    std::fs::write(d.join("app.cell"), r#"
cell N {
    on longue(n: Int) [native] {
        let sb = strbuf()
        for i in range(0, n) { sb_push_int(sb, i)  sb_push_char(sb, 44) }
        return sb_len(sb)
    }
    on tours(n: Int, fois: Int) {
        let total = 0
        for k in range(0, fois) { total = total + longue(n) }
        return total
    }
}
"#).unwrap();
    // (the first run compiles the library: its memory is the compiler's, not counted)
    let (_, out) = peak_mb(&d, &["run", "app.cell", "tours", "10", "1"]);
    assert!(out.contains("20"), "{out}");
    let (one, _) = peak_mb(&d, &["run", "app.cell", "tours", "4000000", "1"]);
    let (forty, _) = peak_mb(&d, &["run", "app.cell", "tours", "4000000", "40"]);
    assert!(forty < one * 3, "forty native strings held {forty} MB, one {one} MB");
}
