//! Scopes, interpolated effects, agent-memory isolation and proof coverage.
use std::process::Command;

fn run(name: &str, source: &str, command: &str, strict: bool) -> (bool, String) {
    let dir = std::env::temp_dir().join(format!("soma_scope_{}_{}", std::process::id(), name));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("app.cell"), source).unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_soma"));
    cmd.args([command, "app.cell"]).current_dir(&dir);
    if strict { cmd.arg("--strict"); }
    let out = cmd.output().unwrap();
    std::fs::remove_dir_all(dir).unwrap();
    (out.status.success(), format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)))
}
macro_rules! case {
    ($name:ident, $source:expr) => {
        #[test]
        fn $name() {
            let (ok, out) = run(stringify!($name), $source, "test", false);
            assert!(ok, "{out}");
        }
    };
}

case!(if_expression_then_bindings_stay_in_the_branch, r#"
cell A { on go() { let x=10 let y=if true { let x=20 x+1 } else { 0 } return [x,y] } }
cell test T { rules { assert A.go()==[10,21] } }
"#);


case!(if_expression_else_bindings_stay_in_the_branch, r#"
cell A { on go() { let x=10 let y=if false { 0 } else { let x=30 x+1 } return [x,y] } }
cell test T { rules { assert A.go()==[10,31] } }
"#);


case!(if_expression_restores_bindings_after_a_caught_error, r#"
cell A { on go() { let x=10 let changed=0 let y=try { map("v",if true { let x=20 changed=1 fail("x","bad") } else { 0 }) } return [x,changed,y.kind] } }
cell test T { rules { assert A.go()==[10,1,"x"] } }
"#);


case!(if_expression_can_return_a_closure_without_leaking_its_locals, r#"
cell A { on go() { let x=10 let f=if true { let x=20 n => n+x } else { n => n } return [x,f(2)] } }
cell test T { rules { assert A.go()==[10,22] } }
"#);


case!(lambda_captures_all_interpolated_operands, r#"
cell A { on go() { let offset=4 let f=x => "{x + offset}" let g=x => "{(x + offset) * 2}" return [f(3),g(3)] } }
cell test T { rules { assert A.go()==["7","14"] } }
"#);


case!(lambda_captures_names_in_interpolated_calls_with_quoted_colons, r#"
cell A { on go() { let text="a:b" let f=x => "{split(text, \":\")[x]}" return f(1) } }
cell test T { rules { assert A.go()=="b" } }
"#);


case!(lambda_captures_interpolated_index_variables, r#"
cell A { on go() { let rows=["a","b"] let i=1 let f=x => "{rows[i]}" return f(0) } }
cell test T { rules { assert A.go()=="b" } }
"#);


case!(lambda_captures_require_error_details, r#"
cell A { on go() { let why="bad input" let f=x => { require x>0 else Bad "reason {why}" x } return try { f(0) } } }
cell test T { rules { assert A.go().kind=="Bad" assert contains(A.go().detail,"reason bad input") } }
"#);


case!(collection_lambdas_start_with_fresh_captures_after_interpolated_assignments, r#"
cell A { on go() { let n=0 return map([1,2],x => "{n}:{if true { n=n+1 n } else { 0 }}") } }
cell test T { rules { assert A.go()==["0:1","0:1"] } }
"#);


case!(self_append_snapshots_before_interpolated_assignments, r#"
cell A { on go() { let xs=[1] xs=push(xs,"{if true { xs=[9] 2 } else { 0 }}") return xs } }
cell test T { rules { assert A.go()==[1,"2"] } }
"#);


case!(nth_snapshots_before_interpolated_assignments, r#"
cell A { on go() { let xs=[1] return nth(xs,to_int("{if true { xs=[9] 0 } else { 0 }}")) } }
cell test T { rules { assert A.go()==1 } }
"#);


case!(map_updates_snapshot_before_interpolated_assignments, r#"
cell A { on go() { let m=map("a",1) m=m |> with("b","{if true { m=map(\"z\",9) 2 } else { 0 }}") return m } }
cell test T { rules { assert A.go()==map("a",1,"b","2") } }
"#);


case!(recall_does_not_read_another_cells_agent_memory, r#"
cell A { on put() { remember("secret",42) } on get() { return recall("secret") } }
cell B { on get() { return recall("secret") } on put() { remember("secret",7) } }
cell test T { rules { let stored=A.put() assert B.get()==() let stored2=B.put() assert A.get()==42 assert B.get()==7 } }
"#);



case!(remember_preserves_json_shaped_strings, r#"
cell A { on roundtrip(v: Any) { remember("k",v) return recall("k") } }
cell test T { rules {
 let object=to_json(map("a",1)) let array=to_json([1,2])
 assert type_of(A.roundtrip(object))=="String" assert A.roundtrip(object)==object
 assert type_of(A.roundtrip(array))=="String" assert A.roundtrip(array)==array
 assert A.roundtrip(map("a",1))==map("a",1) assert A.roundtrip([1,2])==[1,2]
} }
"#);


case!(remember_rejects_functions_before_overwriting_values, r#"
cell A { on put(v: Any) { remember("k",v) } on get() { return recall("k") } }
cell test T { rules { let saved=A.put(7) assert_fails A.put(x => x+1) matching "type" assert A.get()==7 } }
"#);


case!(remember_rejects_functions_nested_in_variants, r#"
cell type Box { variants { Box { value: Any } } }
cell type Tuple { variants { Tuple(Any) } }
cell A { on put(v: Any) { remember("k",v) } }
cell test T { rules { assert_fails A.put(Box { value: [x => x+1] }) matching "type" assert_fails A.put(Tuple(map("f",x => { let y=x+1 y }))) matching "type" } }
"#);


case!(remember_rejects_excessive_list_depth_and_preserves_the_previous_value, r#"
cell A {
 on seed() { remember("k",7) }
 on go(n: Int) { let v=0 for i in range(0,n) { v=[v] } remember("k",v) }
 on get() { return recall("k") }
}
cell test T { rules { let saved=A.seed() assert_fails A.go(101) matching "type" assert A.get()==7 let accepted=A.go(100) assert type_of(A.get())=="List" } }
"#);


case!(remember_counts_variant_and_escaped_map_storage_layers, r#"
cell type Box { variants { Box { value: Any } } }
cell A {
 on records() { let v=0 for i in range(0,34) { v=Box { value: v } } remember("k",v) }
 on maps() { let v=0 for i in range(0,51) { v=map("__wrapped",v) } remember("k",v) }
}
cell test T { rules { assert_fails A.records() matching "type" assert_fails A.maps() matching "type" } }
"#);

#[test]
fn termination_analysis_sees_quoted_colons_in_interpolation() {
    let (ok, out) = run("termination_analysis_sees_quoted_colons_in_interpolation", r#"cell A { on go(x: String) { return "{go(\"x:y\")}" } }"#, "verify", true);
    assert!(!ok && out.contains("recursive"), "{out}");
}

#[test]
fn invariant_analysis_sees_interpolated_writes_with_quoted_colons() {
    let (ok, out) = run("invariant_analysis_sees_interpolated_writes_with_quoted_colons", r#"cell A { memory { m: Map<String,Int> invariant m >= 0 } on go(n: Int) { return "{m.set(\"a:b\",n)}" } }"#, "verify", true);
    assert!(!ok && out.contains("runtime-checked"), "{out}");
}

#[test]
fn cost_analysis_sees_interpolated_think_with_quoted_colons() {
    let (ok, out) = run("cost_analysis_sees_interpolated_think_with_quoted_colons", r#"cell agent A { cost { tokens: 1 } on go() { return "{think(\"a:b\",map(\"max_tokens\",2))}" } }"#, "check", false);
    assert!(!ok && out.contains("tokens"), "{out}");
}

#[test]
fn branch_locals_are_not_visible_after_an_if_expression() {
    let (ok, out) = run("branch_escape", r#"cell A { on go() { let y=if true { let branch=1 branch } else { 0 } return branch } }"#, "check", false);
    assert!(!ok && out.contains("branch"), "{out}");
}

#[test]
fn branch_locals_are_not_visible_in_the_other_if_branch() {
    let (ok, out) = run("branch_other", r#"cell A { on go(flag: Bool) { return if flag { let branch=1 branch } else { branch } } }"#, "check", false);
    assert!(!ok && out.contains("branch"), "{out}");
}

#[test]
fn remembered_strings_survive_a_process_restart_with_their_type() {
    let dir = std::env::temp_dir().join(format!("soma_scope_{}_persist", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("app.cell"), r#"
cell A {
 on put() { remember("k",to_json(map("sensor",7))) }
 on inspect() { return type_of(recall("k")) }
}
"#).unwrap();
    let invoke = |handler: &str| Command::new(env!("CARGO_BIN_EXE_soma"))
        .args(["run", "app.cell", handler]).current_dir(&dir).output().unwrap();
    let put = invoke("put");
    assert!(put.status.success(), "{}", String::from_utf8_lossy(&put.stderr));
    let read = invoke("inspect");
    let output = format!("{}{}", String::from_utf8_lossy(&read.stdout), String::from_utf8_lossy(&read.stderr));
    std::fs::remove_dir_all(dir).unwrap();
    assert!(read.status.success() && output.contains("String"), "{output}");
}
