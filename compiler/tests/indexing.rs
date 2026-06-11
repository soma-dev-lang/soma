//! V1.9 data-structure ergonomics: bracket indexing (read + write) on
//! lists/maps/strings, `with(list, i, v)`, and stepped/descending
//! `range(start, end, step)`.

use std::process::Command;

fn run(src: &str, sig: &str) -> (String, i32) {
    // hash the source so parallel tests never collide on the temp file
    let h: u64 = src.bytes().fold(1469598103934665603u64, |a, b| (a ^ b as u64).wrapping_mul(1099511628211));
    let path = std::env::temp_dir().join(format!("v19_{sig}_{h:x}.cell"));
    std::fs::write(&path, src).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_soma"))
        .args(["run", path.to_str().unwrap(), sig])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("run soma");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (text, out.status.code().unwrap_or(-1))
}

#[test]
fn list_index_read_and_write() {
    let (o, c) = run(
        "cell T { face { signal go() -> Int } on go() { let xs = list(5,6,7) xs[2] = 99 return xs[2] } }",
        "go",
    );
    assert_eq!(c, 0, "{o}");
    assert!(o.contains("99"), "{o}");
}

#[test]
fn map_index_read_and_write() {
    let (o, c) = run(
        "cell T { face { signal go() -> Int } on go() { let m = map(\"a\", 1) m[\"a\"] = 42 return m[\"a\"] } }",
        "go",
    );
    assert_eq!(c, 0, "{o}");
    assert!(o.contains("42"), "{o}");
}

#[test]
fn string_index_returns_char() {
    let (o, c) = run(
        "cell T { face { signal go() -> String } on go() { let s = \"abc\" return s[1] } }",
        "go",
    );
    assert_eq!(c, 0, "{o}");
    assert!(o.trim().ends_with('b'), "{o}");
}

#[test]
fn list_index_out_of_bounds_errors() {
    let (o, c) = run(
        "cell T { face { signal go() -> Int } on go() { let xs = list(1,2) return xs[5] } }",
        "go",
    );
    assert_eq!(c, 1, "out-of-bounds must error: {o}");
    assert!(o.contains("out of bounds"), "{o}");
}

#[test]
fn with_replaces_list_element_functionally() {
    let (o, c) = run(
        "cell T { face { signal go() -> List } on go() { return with(list(1,2,3), 1, 88) } }",
        "go",
    );
    assert_eq!(c, 0, "{o}");
    assert!(o.contains("[1, 88, 3]"), "{o}");
}

#[test]
fn descending_range() {
    let (o, c) = run(
        "cell T { face { signal go() -> String } on go() { let s = \"\" for r in range(3, 0 - 1, 0 - 1) { s = \"{s}{r}\" } return s } }",
        "go",
    );
    assert_eq!(c, 0, "{o}");
    assert!(o.contains("3210"), "{o}");
}

#[test]
fn stepped_range() {
    let (o, c) = run(
        "cell T { face { signal go() -> String } on go() { let s = \"\" for r in range(0, 10, 2) { s = \"{s}{r} \" } return s } }",
        "go",
    );
    assert_eq!(c, 0, "{o}");
    assert!(o.contains("0 2 4 6 8"), "{o}");
}

#[test]
fn indexing_inside_string_interpolation() {
    let (o, c) = run(
        "cell T { face { signal go() -> String } on go() { let xs = list(7,8,9) return \"mid={xs[1]}\" } }",
        "go",
    );
    assert_eq!(c, 0, "{o}");
    assert!(o.contains("mid=8"), "{o}");
}

#[test]
fn index_assign_into_undefined_errors() {
    let (o, c) = run(
        "cell T { face { signal go() -> Int } on go() { nope[0] = 1 return 0 } }",
        "go",
    );
    assert_eq!(c, 1, "writing into an undefined var must error: {o}");
}

#[test]
fn numeric_reductions_over_lists() {
    let (o, c) = run(
        "cell T { face { signal go() -> String } on go() { let xs = list(3,7,2,9) return \"{sum(xs)} {product(xs)} {min(xs)} {max(xs)} {avg(xs)}\" } }",
        "go",
    );
    assert_eq!(c, 0, "{o}");
    assert!(o.contains("21 378 2 9 5"), "{o}");
}

#[test]
fn sum_promotes_to_float() {
    let (o, c) = run(
        "cell T { face { signal go() -> Float } on go() { return sum(list(1.5, 2.5, 3.0)) } }",
        "go",
    );
    assert_eq!(c, 0, "{o}");
    assert!(o.contains("7"), "{o}");
}

#[test]
fn coalesce_with_index_access() {
    let (o, c) = run(
        "cell T { face { signal go() -> String } on go() { let m = map(\"x\", 5) return \"{m[\\\"x\\\"] ?? 9} {m[\\\"y\\\"] ?? 9}\" } }",
        "go",
    );
    assert_eq!(c, 0, "{o}");
    assert!(o.contains("5 9"), "{o}");
}
