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

#[test]
fn dot_field_assignment_on_map() {
    let (o, c) = run(
        "cell T { face { signal go() -> String } on go() { let g = map(\"bet\", 0) g.bet = 10 g.bet = g.bet + 5 return \"{g.bet}\" } }",
        "go",
    );
    assert_eq!(c, 0, "{o}");
    assert!(o.contains("15"), "{o}");
}

#[test]
fn dot_field_read_write_on_slot_is_symmetric() {
    let (o, c) = run(
        "cell T { face { signal go() -> Int } memory { box: Map<String, Int> [ephemeral, local] } on go() { box.score = 42 return box.score } }",
        "go",
    );
    assert_eq!(c, 0, "{o}");
    assert!(o.contains("42"), "{o}");
}

#[test]
fn dot_method_call_still_parses() {
    // ensure the field-set lookahead didn't break `slot.set(k, v)`
    let (o, c) = run(
        "cell T { face { signal go() -> Int } memory { m: Map<String, Int> [ephemeral, local] } on go() { m.set(\"k\", 7) return m.get(\"k\") } }",
        "go",
    );
    assert_eq!(c, 0, "{o}");
    assert!(o.contains("7"), "{o}");
}

#[test]
fn record_literal_construct_and_mutate() {
    let (o, c) = run(
        "cell T { face { signal go() -> String } on go() { let g = Game { bet: 10, pot: 5 } g.bet = 20 g.pot = g.pot + g.bet return \"{g.bet} {g.pot} {is_a(g, \\\"Game\\\")}\" } }",
        "go",
    );
    assert_eq!(c, 0, "{o}");
    assert!(o.contains("20 25 true"), "{o}");
}

#[test]
fn list_constructs_nested_not_spread() {
    let (o, c) = run(
        "cell T { face { signal go() -> String } on go() { let xs = list(list(1,2), list(3,4)) return \"{len(xs)} {xs[1][0]}\" } }",
        "go",
    );
    assert_eq!(c, 0, "{o}");
    assert!(o.contains("2 3"), "nested list must not spread: {o}");
}

#[test]
fn nested_lvalue_field_then_index() {
    let (o, c) = run(
        "cell T { face { signal go() -> String } on go() { let g = Game { board: list(1,2,3) } g.board[0] = 99 return \"{g.board}\" } }",
        "go",
    );
    assert_eq!(c, 0, "{o}");
    assert!(o.contains("[99, 2, 3]"), "{o}");
}

#[test]
fn nested_lvalue_index_then_index() {
    let (o, c) = run(
        "cell T { face { signal go() -> String } on go() { let xs = list(list(1,2), list(3,4)) xs[1][0] = 88 return \"{xs}\" } }",
        "go",
    );
    assert_eq!(c, 0, "{o}");
    assert!(o.contains("[[1, 2], [88, 4]]"), "{o}");
}

#[test]
fn record_round_trips_through_a_slot() {
    let (o, c) = run(
        "cell T { face { signal go() -> String } memory { players: Map<String, Map> [ephemeral, local] } on go() { players.set(\"p1\", Player { name: \"ana\", chips: 1000 }) let p = players.get(\"p1\") p.chips = p.chips - 200 players.set(\"p1\", p) let back = players.get(\"p1\") return \"{back.name} {back.chips}\" } }",
        "go",
    );
    assert_eq!(c, 0, "{o}");
    assert!(o.contains("ana 800"), "{o}");
}

#[test]
fn matrix_reshape_transpose_shape_via_methods() {
    let (o, c) = run(
        "cell T { face { signal go() -> String } on go() { let M = [1,2,3,4,5,6].reshape(2,3) return \"{M.shape} {M.T.shape}\" } }",
        "go",
    );
    assert_eq!(c, 0, "{o}");
    assert!(o.contains("[2, 3] [3, 2]"), "{o}");
}

#[test]
fn matrix_multiply_operator() {
    let (o, c) = run(
        "cell T { face { signal go() -> String } on go() { let A = [1,2,3,4].reshape(2,2) let B = [5,6,7,8].reshape(2,2) return to_string(A * B) } }",
        "go",
    );
    assert_eq!(c, 0, "{o}");
    assert!(o.contains("[[19.0, 22.0], [43.0, 50.0]]"), "{o}");
}

#[test]
fn matrix_scalar_and_elementwise() {
    let (o, c) = run(
        "cell T { face { signal go() -> String } on go() { let A = [1,2,3,4].reshape(2,2) return \"{2 * A} {A + A}\" } }",
        "go",
    );
    assert_eq!(c, 0, "{o}");
    assert!(o.contains("[[2.0, 4.0], [6.0, 8.0]] [[2.0, 4.0], [6.0, 8.0]]"), "{o}");
}

#[test]
fn matrix_det() {
    let (o, c) = run(
        "cell T { face { signal go() -> Float } on go() { return det([4,3,6,3].reshape(2,2)) } }",
        "go",
    );
    assert_eq!(c, 0, "{o}");
    // det([4,3;6,3]) = 4*3 - 3*6 = -6
    assert!(o.contains("-6"), "{o}");
}

#[test]
fn ufcs_list_methods() {
    let (o, c) = run(
        "cell T { face { signal go() -> String } on go() { let xs = [3,1,2] return \"{xs.sum()} {xs.sort()} {xs.reverse()}\" } }",
        "go",
    );
    assert_eq!(c, 0, "{o}");
    assert!(o.contains("6 [1, 2, 3] [2, 1, 3]"), "{o}");
}

#[test]
fn flat_list_concat_unchanged_by_matrix_ops() {
    let (o, c) = run(
        "cell T { face { signal go() -> List } on go() { return [1,2] + [3,4] } }",
        "go",
    );
    assert_eq!(c, 0, "{o}");
    assert!(o.contains("[1, 2, 3, 4]"), "flat-list + must still concat: {o}");
}
