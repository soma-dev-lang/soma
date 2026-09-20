//! Regression coverage for evaluation order, dispatch and constructors.
use std::process::Command;
fn passes(name: &str, source: &str) {
    let dir = std::env::temp_dir().join(format!("soma_execution_{}_{}", std::process::id(), name));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("app.cell"), source).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_soma"))
        .args(["test", "app.cell"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    std::fs::remove_dir_all(dir).unwrap();
}
macro_rules! case {
    ($name:ident,$source:expr) => {
        #[test]
        fn $name() {
            passes(stringify!($name), $source);
        }
    };
}
case!(
    append_arguments_can_read_the_original_list,
    r#"
cell A {
 on run() { let xs=[1,2] xs=push(xs,len(xs),xs[0]) return xs }
 on refused() { let xs=[1,2] let e=try { if true { xs=push(xs,3,fail("late","boom")) 0 } else { 0 } } return xs }
 on legacy() { let xs=[1,2] xs=list(xs,len(xs)) return xs }
 on snapshot() { let xs=[1] xs=push(xs,if true { xs=[9] 2 } else { 0 }) return xs }
}
cell test T { rules {
 assert A.run()==[1,2,2,1]
 assert A.refused()==[1,2]
 assert A.legacy()==[1,2,2]
 assert A.snapshot()==[1,2]
} }
"#
);

case!(
    append_assignment_respects_user_handlers,
    r#"
cell A {
 on push(xs: List, x: Int) { return [99,x] }
 on list(xs: List, x: Int) { return [88,x] }
 on run() { let xs=[1] xs=push(xs,2) return xs }
 on legacy() { let xs=[1] xs=list(xs,2) return xs }
}
cell test T { rules { assert A.run()==[99,2] assert A.legacy()==[88,2] } }
"#
);

case!(
    arithmetic_assignment_evaluates_rhs_once,
    r#"
cell A {
 memory { calls: Map<String,Int> }
 on number(n: Any) { calls.set("n",(calls.get("n") ?? 0)+1) return n }
 on run(mode: Int) {
  calls.set("n",0)
  let x=7
  if mode==0 { x=x+number(0.5) }
  if mode==1 { x=x-number(0.5) }
  if mode==2 { x=x*number(0.5) }
  if mode==3 { x=x/number(2) }
  if mode==4 { x=x % number(2) }
  if mode==5 { x=x/number(2.0) }
  return map("value",x,"calls",calls.get("n"))
 }
}
cell test T { rules {
 assert A.run(0)==map("value",7.5,"calls",1)
 assert A.run(1)==map("value",6.5,"calls",1)
 assert A.run(2)==map("value",3.5,"calls",1)
 assert A.run(3)==map("value",3.5,"calls",1)
 assert A.run(4)==map("value",1,"calls",1)
 assert A.run(5)==map("value",3.5,"calls",1)
} }
"#
);

case!(
    logical_assignment_checks_left_operand_before_rhs,
    r#"
cell A {
 on run() { let x=1 x=x && fail("unexpected","rhs ran") return x }
}
cell test T { rules { assert_fails A.run() matching "type" } }
"#
);

case!(
    map_assignment_applies_every_pair,
    r#"
cell A {
 on snapshot() { let m=map("a",1) m=m |> with("b",if true { m=map("z",9) 2 } else { 0 }) return m }
 on run() { let m=map("a",1) m=m |> with("b",2,"c",3,"a",4) return m }
}
cell test T { rules { assert A.run()==map("a",4,"b",2,"c",3) assert A.snapshot()==map("a",1,"b",2) } }
"#
);

case!(
    map_assignment_evaluates_later_arguments_and_refuses_failure,
    r#"
cell A {
 on run() { let m=map() m=m |> with("a",1,"b",fail("late","boom")) return m }
}
cell test T { rules { assert_fails A.run() matching "late" } }
"#
);

case!(
    map_and_record_updates_require_complete_pairs,
    r#"
cell type R { variants { R { x: Int } } }
cell A { on run() { let m=map() m=m |> with("a",1,"dangling") return m } }
cell test T { rules {
 assert_fails with(map(),"dangling") matching "type"
 assert_fails with(map(),"a",1,"dangling") matching "type"
 assert_fails with(R { x: 1 },"x",2,"dangling") matching "type"
 assert_fails A.run() matching "type"
 assert with(map("a",1))==map("a",1)
} }
"#
);

case!(
    update_fallback_evaluates_arguments_once,
    r#"
cell type R { variants { R { x: Int } } }
cell A {
 memory { calls: Map<String,Int> }
 on value() { let n=(calls.get("n") ?? 0)+1 calls.set("n",n) return n }
 on run() { let r=R { x: 0 } r=r |> with("x",value()) return r.x }
}
cell test T { rules { assert A.run()==1 } }
"#
);

case!(
    map_assignment_respects_user_handler,
    r#"
cell A {
 on with(m: Map,k: String,v: Int) { return map("custom",v) }
 on run() { let m=map() m=m |> with("x",7) return m }
}
cell test T { rules { assert A.run()==map("custom",7) } }
"#
);

case!(
    local_len_callable_overrides_builtin_optimization,
    r#"
cell A {
 on run() { let len=x => 77 let xs=[1,2] return len(xs) }
}
cell test T { rules { assert A.run()==77 } }
"#
);

case!(
    nth_does_not_retry_an_invalid_index_expression,
    r#"
cell A {
 memory { calls: Map<String,Int> }
 on index() { let n=(calls.get("n") ?? 0)+1 calls.set("n",n) return if n==1 { "bad" } else { 0 } }
 on run() { let xs=[5] return nth(xs,index()) }
}
cell test T { rules { assert_fails A.run() matching "type" } }
"#
);

case!(
    nth_preserves_left_to_right_argument_snapshots,
    r#"
cell A {
 on run() { let xs=[5] return nth(xs,if true { xs=[9] 0 } else { 0 }) }
}
cell test T { rules { assert A.run()==5 } }
"#
);

case!(
    optimized_reads_honor_builtin_mocks,
    r#"
cell A {
 on size() { let xs=[1,2] return len(xs) }
 on item() { let xs=[1,2] return nth(xs,0) }
}
cell test T { rules {
 mock len 77
 mock nth 88
 assert A.size()==77 assert A.item()==88
} }
"#
);

case!(
    optimized_writes_honor_builtin_mocks,
    r#"
cell A {
 on append_item() { let xs=[1] xs=push(xs,2) return xs }
 on update() { let m=map() m=m |> with("x",1) return m }
}
cell test T { rules {
 mock push [[77]]
 mock with map("mocked",88)
 assert A.append_item()==[77] assert A.update()==map("mocked",88)
} }
"#
);

case!(
    optimized_range_loop_honors_builtin_mock,
    r#"
cell A { on run() { let total=0 for n in range(0,2) { total=total+n } return total } }
cell test T { rules { mock range [[4,7]] assert A.run()==11 } }
"#
);

case!(
    pipes_call_local_lambdas,
    r#"
cell A {
 on run() { let f=x => x+7 return 3 |> f }
 on call() { let f=x => { let y=x+7 y } return 3 |> f() }
}
cell test T { rules { assert A.run()==10 assert A.call()==10 } }
"#
);

case!(
    pipes_prefer_the_calling_cells_handler,
    r#"
cell Foreign { on transform(x: Int) { return 99 } }
cell A {
 on transform(x: Int) { return x+1 }
 on run() { return 3 |> transform }
 on call() { return 3 |> transform() }
}
cell test T { rules { assert A.run()==4 assert A.call()==4 } }
"#
);

case!(
    record_constructors_normalize_declared_float_fields,
    r#"
cell type R { variants { R { x: Float, xs: List<Float> } } }
cell test T { rules {
 let r=R { x: 3, xs: [1,2] }
 assert type_of(r.x)=="Float"
 assert type_of(r.xs[0])=="Float"
 assert_fails R { x: ipow(10,400), xs: [] } matching "Float range"
} }
"#
);

case!(
    tuple_constructors_normalize_declared_fields,
    r#"
cell type R { variants { R { x: Float } } }
cell type Event { variants { Reading(Float) Wrapped(R) } }
cell A {
 on extract(e: Event) { return match e { Reading(x) -> type_of(x) Wrapped(r) -> type_of(r.x) } }
}
cell test T { rules {
 assert A.extract(Reading(3))=="Float"
 assert A.extract(Wrapped(map("x",3)))=="Float"
 assert_fails Reading(ipow(10,400)) matching "Float range"
} }
"#
);

case!(
    record_constructor_coerces_nested_record_inputs,
    r#"
cell type R { variants { R { x: Float } } }
cell type Batch { variants { Batch { rows: List<R> } } }
cell test T { rules {
 let b=Batch { rows: [map("x",3)] }
 assert type_of(b.rows[0])=="Variant"
 assert type_of(b.rows[0].x)=="Float"
} }
"#
);

case!(
    declared_variant_cannot_be_constructed_with_wrong_shape,
    r#"
cell type Event { variants { Reading(Int) Ready } }
cell test T { rules {
 assert_fails Reading { x: 3 } matching "type"
 assert_fails Ready { x: 3 } matching "type"
 assert type_of(Unknown { x: 3 })=="Map"
} }
"#
);

case!(
    block_lambdas_work_in_ordinary_collection_calls,
    r#"
cell test T { rules {
 assert map([1,2],x => { let y=x+1 y })==[2,3]
 assert filter([1,2,3],x => { let keep=x>1 keep })==[2,3]
} }
"#
);

case!(
    record_updates_honor_builtin_mocks,
    r#"
cell type R { variants { R { x: Int } } }
cell test T { rules {
 mock with map("mocked",77)
 assert with(R { x: 1 },"x",2)==map("mocked",77)
} }
"#
);

case!(
    map_keys_are_not_restricted_to_machine_integer_indices,
    r#"
cell A {
 on update(k: Int) { let m=map() m=m |> with(k,7) return m }
}
cell test T { rules {
 let k=ipow(10,50)
 assert with(map(),k,7)==map(k,7)
 assert A.update(k)==map(k,7)
 assert_fails with([1],k,7) matching "range"
} }
"#
);
