//! Invalid values must fail at typed and persistent boundaries, before effects.
use std::process::Command;

fn passes(name: &str, source: &str) {
    let dir = std::env::temp_dir().join(format!("soma_inputs_{}_{}", std::process::id(), name));
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
    ($name:ident, $source:expr) => {
        #[test]
        fn $name() {
            passes(stringify!($name), $source);
        }
    };
}

case!(
    float_parameter_refuses_overflow_before_handler_effects,
    r#"
cell A {
 memory { seen: Map<String, Int> }
 on accept(x: Float) { seen.set("called",1) return x }
 on count() { return seen.get("called") }
}
cell test T { rules {
 assert_fails A.accept(ipow(10,400)) matching "Float range"
 assert A.count()==()
 assert A.accept(3)==3.0
 assert type_of(A.accept(3))=="Float"
 assert A.accept(ipow(10,308))==1e308
} }
"#
);

case!(
    nested_float_parameters_refuse_overflow,
    r#"
cell A { on accept(xs: List<Map<String, Float>>) { return xs } }
cell test T { rules {
 assert_fails A.accept([map("x",ipow(10,400))]) matching "Float range"
 assert type_of(A.accept([map("x",3)])[0].x)=="Float"
} }
"#
);

case!(
    record_input_refuses_float_overflow,
    r#"
cell type Reading { variants { Reading { value: Float } } }
cell A { on accept(r: Reading) { return r.value } }
cell test T { rules {
 assert_fails A.accept(map("value",ipow(10,400))) matching "Float range"
 assert_fails A.accept(to_json(map("value",ipow(10,400)))) matching "Float range"
 assert type_of(A.accept(map("value",3)))=="Float"
} }
"#
);

case!(
    record_update_refuses_float_overflow,
    r#"
cell type Reading { variants { Reading { value: Float } } }
cell A {
 on mutate() { let r=Reading { value: 2.0 } r.value=ipow(10,400) return r }
}
cell test T { rules {
 let r=Reading { value: 2.0 }
 assert_fails with(r,"value",ipow(10,400)) matching "Float range"
 assert_fails A.mutate() matching "Float range"
 assert r.value==2.0
} }
"#
);

case!(
    float_slot_refuses_overflow_and_preserves_previous_value,
    r#"
cell A {
 memory { readings: Map<String, Float> }
 on put(x: Any) { readings.set("x",x) }
 on get() { return readings.get("x") }
}
cell test T { rules {
 let initial=A.put(2)
 assert_fails A.put(ipow(10,400)) matching "Float range"
 assert A.get()==2.0
 assert type_of(A.get())=="Float"
} }
"#
);

case!(
    typed_map_parameters_check_underscore_keys,
    r#"
cell A { on accept(m: Map<String, Int>) { return m } }
cell test T { rules {
 assert_fails A.accept(map("_speed","bad")) matching "type"
 assert_fails A.accept(map("__private",())) matching "type"
 assert_fails A.accept(map("_type","metadata is still data")) matching "type"
 assert A.accept(map("_speed",7))._speed==7
} }
"#
);

case!(
    nested_typed_maps_check_underscore_keys,
    r#"
cell A { on accept(xs: List<Map<String, List<Int>>>) { return xs } }
cell test T { rules {
 assert_fails A.accept([map("_speed",["bad"])]) matching "type"
 assert A.accept([map("_speed",[7])])[0]._speed[0]==7
} }
"#
);

case!(
    typed_map_return_checks_underscore_keys,
    r#"
cell A {
 face { signal result() -> Map<String, Int> }
 on result() { return map("_speed","bad") }
}
cell test T { rules { assert_fails A.result() matching "type" } }
"#
);

case!(
    typed_maps_check_variant_payloads_at_every_boundary,
    r#"
cell type Row { variants { Row { value: Any } } }
cell type Tupled { variants { Tupled(Any) } }
cell A {
 memory { rows: Map<String, Map<String, Int>> }
 face { signal result(x: Any) -> Map<String, Int> }
 on accept(x: Map<String, Int>) { return x }
 on result(x: Any) { return x }
 on put(x: Any) { rows.set("x",x) }
 on get() { return rows.get("x") }
}
cell test T { rules {
 let bad=Row { value: "bad" }
 assert_fails A.accept(bad) matching "type"
 assert_fails A.result(bad) matching "type"
 assert_fails A.put(bad) matching "type"
 assert_fails A.accept(Tupled("bad")) matching "type"
 assert A.get()==()
 assert A.accept(Row { value: 3 }).value==3
 assert A.result(Row { value: 3 }).value==3
 let valid=A.put(Row { value: 3 })
 assert A.get().value==3
} }
"#
);

case!(
    typed_map_slot_checks_underscore_keys_and_rolls_back,
    r#"
cell A {
 memory { readings: Map<String, Map<String, Int>> }
 on put(x: Map) { readings.set("x",x) }
 on get() { return readings.get("x") }
}
cell test T { rules {
 let initial=A.put(map("_speed",3))
 assert_fails A.put(map("_speed","bad")) matching "type"
 assert A.get()._speed==3
} }
"#
);

case!(
    record_map_fields_check_underscore_keys,
    r#"
cell type Reading { variants { Reading { values: Map<String, Int> } } }
cell A { on accept(x: Map) { return Reading { values: x } } }
cell test T { rules {
 assert_fails A.accept(map("_speed","bad")) matching "type"
 assert A.accept(map("_speed",3)).values._speed==3
} }
"#
);

case!(
    local_nth_has_the_same_index_limits_as_expression_nth,
    r#"
cell test T { rules {
 let xs=[4,5]
 let huge=ipow(10,50)
 assert_fails nth([4,5],huge) matching "range"
 assert_fails nth(xs,huge) matching "range"
 assert_fails nth(xs,-huge) matching "range"
 assert nth(xs,-1)==5
 assert nth(xs,9223372036854775807)==()
 assert nth(xs,-9223372036854775808)==()
} }
"#
);

case!(
    sleep_rejects_noninteger_durations,
    r#"
cell test T { rules {
 assert_fails sleep(0.25) matching "type"
 assert_fails sleep(-0.25) matching "type"
 assert_fails sleep("invalid") matching "type"
 assert_fails sleep(false) matching "type"
 assert_fails sleep(()) matching "type"
 assert_fails sleep(sqrt(-1.0)) matching "type"
 assert_fails sleep(-1) matching "range"
 assert sleep(0)==()
} }
"#
);

case!(
    record_payload_cannot_hide_a_function_in_persistent_data,
    r#"
cell type Wrapped { variants { Wrapped { value: Any } } }
cell type Tupled { variants { Tupled(Any) } }
cell A {
 memory { values: Map<String, Any> }
 on put(x: Any) { values.set("x",x) }
 on get() { return values.get("x") }
}
cell test T { rules {
 let initial=A.put(3)
 assert_fails A.put(Wrapped { value: x => x+1 }) matching "cannot be stored"
 assert_fails A.put(Tupled(x => x+1)) matching "cannot be stored"
 assert_fails A.put([Wrapped { value: map("f", x => x+1) }]) matching "cannot be stored"
 assert A.get()==3
 let valid=A.put(Wrapped { value: 4 })
 assert A.get().value==4
} }
"#
);

case!(
    declared_sum_return_rejects_values_of_another_type,
    r#"
cell type Status { variants { Ready  Fault { reason: String } } }
cell type Other { variants { Wrong } }
cell A {
 memory { seen: Map<String, Int> }
 face { signal result(x: Any) -> Status }
 on result(x: Any) { seen.set("called",1) return x }
 on count() { return seen.get("called") }
}
cell test T { rules {
 assert_fails A.result(Wrong) matching "type"
 assert_fails A.result(42) matching "type"
 assert A.count()==()
 assert A.result(Ready)==Ready
 assert A.result(Fault { reason: "sensor" }).reason=="sensor"
} }
"#
);

fn persistent_depth(name: &str, wrapper: &str, valid: usize, invalid: usize) {
    let dir = std::env::temp_dir().join(format!("soma_depth_{}_{}", std::process::id(), name));
    std::fs::create_dir_all(&dir).unwrap();
    let source = format!(
        r#"
cell type Wrapped {{ variants {{ Wrapped {{ value: Any }} }} }}
cell type Tupled {{ variants {{ Tupled(Any) }} }}
cell A {{
 memory {{ values: Map<String, Any> [persistent] }}
 on nested(n: Int) {{ let x=0 for i in range(0,n) {{ x={wrapper} }} return x }}
 on put(n: Int) {{ values.set("x",nested(n)) return true }}
 on read(n: Int) {{ return values.get("x")==nested(n) }}
}}
"#
    );
    std::fs::write(dir.join("app.cell"), source).unwrap();
    let run = |handler: &str, n: usize| {
        Command::new(env!("CARGO_BIN_EXE_soma"))
            .args(["run", "app.cell", handler, &n.to_string()])
            .current_dir(&dir)
            .output()
            .unwrap()
    };
    let first = run("put", valid);
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let read = run("read", valid);
    assert!(
        read.status.success() && String::from_utf8_lossy(&read.stdout).trim() == "true",
        "{read:?}"
    );
    for n in [valid + 1, invalid] {
        let refused = run("put", n);
        assert!(
            !refused.status.success(),
            "oversized encoded value was accepted"
        );
        assert!(
            String::from_utf8_lossy(&refused.stderr).contains("cannot be stored"),
            "{refused:?}"
        );
    }
    let read = run("read", valid);
    assert!(
        read.status.success() && String::from_utf8_lossy(&read.stdout).trim() == "true",
        "{read:?}"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn persistent_records_count_encoding_depth_before_commit() {
    persistent_depth("record", "Wrapped { value: x }", 33, 44);
}

#[test]
fn persistent_tuple_variants_count_encoding_depth_before_commit() {
    persistent_depth("tuple", "Tupled(x)", 50, 65);
}

#[test]
fn persistent_escaped_maps_count_encoding_depth_before_commit() {
    persistent_depth("escaped_map", "map(\"__value\",x)", 50, 65);
}
