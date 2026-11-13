//! `soma skeleton`: a program as its structure and frames, no bodies
//! (SKELETON_DESIGN.md, phase 1 — frames are inferred from the bodies).

use std::process::Command;

fn soma(args: &[&str]) -> (String, i32) {
    let o = Command::new(env!("CARGO_BIN_EXE_soma")).args(args).output().expect("failed to run soma");
    (
        format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)),
        o.status.code().unwrap_or(-1),
    )
}

fn program(name: &str, source: &str) -> String {
    let dir = std::env::temp_dir().join(format!("soma_skeleton_{}", name));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("app.cell");
    std::fs::write(&path, source).unwrap();
    path.display().to_string()
}

const SHOP: &str = r#"
cell Stock {
    face {
        signal level(sku: String) -> Int
        signal take(sku: String, n: Int) -> Bool
    }
    memory {
        items: Map<String, Int> [persistent]
        invariant items >= 0
    }
    on level(sku: String) {
        return items.get(sku) ?? 0
    }
    on take(sku: String, n: Int) {
        let have = level(sku)
        if have < n { return false }
        items.set(sku, have - n)
        return true
    }
}

cell Orders {
    face {
        signal place(id: String, sku: String) -> String
        signal cancel(id: String) -> String
        signal status(id: String) -> String
        signal quote(n: Int) -> Int
    }
    memory {
        log: Map<String, String> [persistent]
        seq: Map<String, Int> [persistent]
    }
    state flow {
        initial: placed
        placed -> packed { guard { n <= 10 } }
        placed -> cancelled
        packed -> shipped
    }
    on place(id: String, sku: String) {
        let n = 1
        if take(sku, n) {
            transition(id, "packed")
            remember_order(id, sku)
            return "packed"
        }
        return cancel(id)
    }
    on cancel(id: String) {
        transition(id, "placed", "cancelled")
        return "cancelled"
    }
    on status(id: String) {
        return get_status(id)
    }
    on quote(n: Int) {
        return n * 3
    }
    on remember_order(id: String, sku: String) {
        log.set(id, sku)
        bump()
    }
    on bump() {
        seq.set("n", (seq.get("n") ?? 0) + 1)
    }
    on audit() {
        return now()
    }
}
"#;

#[test]
fn the_map_gives_every_signal_its_frame() {
    let path = program("map", SHOP);
    let (out, code) = soma(&["skeleton", &path]);
    assert_eq!(code, 0, "{out}");
    // structure is kept, guard text included
    assert!(out.contains("items: Map<String, Int> [persistent]"), "{out}");
    assert!(out.contains("invariant items >= 0"), "{out}");
    assert!(out.contains("placed -> packed { guard { n <= 10 } }"), "{out}");
    // a read, and a write that goes through another signal of the cell
    assert!(out.contains("signal level(sku: String) -> Int  reads items"), "{out}");
    assert!(out.contains("signal take(sku: String, n: Int) -> Bool  writes items  calls level"), "{out}");
    // helpers are folded in: `place` writes what remember_order and bump write
    assert!(out.contains("writes log, seq"), "{out}");
    assert!(out.contains("moves placed -> packed"), "{out}");
    assert!(out.contains("calls Stock.take, cancel"), "{out}");
    assert!(out.contains("// helpers (folded into the frames above): remember_order, bump"), "{out}");
    assert!(!out.contains("signal remember_order"), "a helper is not a bone: {out}");
    // a declared source is the edge; a machine read is a read
    assert!(out.contains("signal cancel(id: String) -> String  moves placed -> cancelled"), "{out}");
    assert!(out.contains("signal status(id: String) -> String  reads flow"), "{out}");
    // a pure signal has no clause at all
    assert!(out.contains("        signal quote(n: Int) -> Int\n"), "{out}");
    // a handler nothing calls is an entry point, and `now` is an effect
    assert!(out.contains("signal audit()  calls now   // not in face"), "{out}");
    // no body made it through
    assert!(!out.contains("have - n") && !out.contains("return"), "{out}");
}

#[test]
fn the_brief_is_what_one_handler_may_touch() {
    let path = program("brief", SHOP);
    let (out, code) = soma(&["skeleton", &path, "Orders.place"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.starts_with("signal place(id: String, sku: String) -> String   // Orders\n"), "{out}");
    assert!(out.contains("log: Map<String, String> [persistent]"), "{out}");
    assert!(out.contains("placed -> packed   guard n <= 10"), "{out}");
    // the contract and the frame of each callee
    assert!(out.contains("Stock.take(sku: String, n: Int) -> Bool"), "{out}");
    assert!(out.contains("writes items  calls level"), "{out}");
    // nothing about the cell's other signals
    assert!(!out.contains("quote") && !out.contains("audit"), "{out}");

    // a slot comes with the invariants that name it
    let (out, _) = soma(&["skeleton", &path, "take"]);
    assert!(out.contains("writes     items: Map<String, Int> [persistent]\n               invariant items >= 0"), "{out}");
    // who relies on it
    assert!(out.contains("called by  Orders.place"), "{out}");

    let (out, _) = soma(&["skeleton", &path, "quote"]);
    assert!(out.contains("pure:"), "{out}");
}

#[test]
fn a_helper_or_an_unknown_signal_is_refused_with_the_way_out() {
    let path = program("refusals", SHOP);
    let (out, code) = soma(&["skeleton", &path, "bump"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("`bump` is a helper of `Orders`"), "{out}");
    let (out, code) = soma(&["skeleton", &path, "Orders.plce"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("Did you mean `Orders.place`?"), "{out}");
}

#[test]
fn json_carries_the_same_frames() {
    let path = program("json", SHOP);
    let (out, code) = soma(&["skeleton", &path, "--json"]);
    assert_eq!(code, 0, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
    let orders = v["cells"].as_array().unwrap().iter().find(|c| c["name"] == "Orders").unwrap();
    let place = orders["signals"].as_array().unwrap().iter().find(|s| s["name"] == "place").unwrap();
    assert_eq!(place["frame"]["writes"], serde_json::json!(["log", "seq"]));
    assert_eq!(place["frame"]["calls"], serde_json::json!(["Stock.take", "cancel"]));
    assert_eq!(place["frame"]["moves"][0], serde_json::json!({ "machine": "", "from": "placed", "to": "packed" }));
    let quote = orders["signals"].as_array().unwrap().iter().find(|s| s["name"] == "quote").unwrap();
    assert_eq!(quote["frame"]["pure"], true);
    assert_eq!(orders["helpers"], serde_json::json!(["remember_order", "bump"]));
}

#[test]
fn a_machine_less_cell_names_the_machine_it_moves() {
    let path = program("foreign", r#"
cell Door {
    face { signal state_of(id: String) -> String }
    state door {
        initial: closed
        closed -> open
        open -> closed
    }
    on state_of(id: String) { return get_status(id) }
}
cell Panel {
    face { signal push(id: String) -> String }
    on push(id: String) {
        transition(id, "open")
        return "open"
    }
}
"#);
    let (out, code) = soma(&["skeleton", &path]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("signal push(id: String) -> String  moves Door.door: closed -> open"), "{out}");
}

// ── phase 2: `skeleton` / `flesh` as source ──────────────────────────

fn project(name: &str, files: &[(&str, &str)]) -> String {
    let dir = std::env::temp_dir().join(format!("soma_skeleton_{}", name));
    let _ = std::fs::remove_dir_all(&dir);
    for (rel, text) in files {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
    }
    dir.join(files[0].0).display().to_string()
}

const DESK_SKELETON: &str = r#"
skeleton Stock {
    memory {
        items: Map<String, Int> [persistent]
        invariant items >= 0
    }
    face {
        signal level(sku: String) -> Int        reads items
        signal take(sku: String, n: Int) -> Bool
            writes items  calls level
    }
}

skeleton Orders {
    memory {
        log: Map<String, String> [persistent]
    }
    state flow {
        initial: placed
        placed -> packed
        placed -> cancelled
        packed -> shipped
    }
    face {
        signal place(id: String, sku: String) -> String
            writes log
            moves placed -> packed
            calls Stock.take, ship
        signal cancel(id: String) -> String     moves placed -> cancelled
        signal ship(id: String) -> String       moves packed -> shipped
        signal status(id: String) -> String     reads flow
        signal quote(n: Int) -> Int
    }
}
"#;

const DESK_FLESH: &str = r#"
flesh Stock {
    on level(sku) {
        return items.get(sku) ?? 0
    }
    on take(sku, n) {
        let have = level(sku)
        if have < n { return false }
        items.set(sku, have - n)
        return true
    }
}

flesh Orders {
    on cancel(id) {
        transition(id, "placed", "cancelled")
        return "cancelled"
    }
    on status(id: String) {
        return get_status(id)
    }
    on quote(n) {
        return _triple(n)
    }
    on _triple(n: Int) {
        return n * 3
    }
}
"#;

#[test]
fn a_skeleton_checks_verifies_and_runs_with_holes() {
    let path = program("holes", &format!("{DESK_SKELETON}{DESK_FLESH}"));
    let (out, code) = soma(&["check", &path]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("2 holes") && out.contains("Orders.place, Orders.ship"), "{out}");

    // the structure is proven before the bodies exist; an edge only a hole
    // will take is waiting, not stale
    let (out, code) = soma(&["verify", &path]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("`place` is a hole — its frame allows {placed → packed}"), "{out}");
    assert!(!out.contains("aspirational"), "{out}");
    assert!(out.contains("VERIFY OK (skeleton — the holes above are open obligations)"), "{out}");

    // a filled signal runs — the flesh left its types out, a helper has its own
    let (out, code) = soma(&["run", &path, "quote", "4"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.trim().ends_with("12"), "{out}");
    // a hole fails by name
    let (out, code) = soma(&["run", &path, "ship", "o1"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("not_implemented") && out.contains("Orders.ship is a hole"), "{out}");
}

#[test]
fn holes_come_in_the_order_to_fill_them() {
    let path = program("order", &format!("{DESK_SKELETON}{DESK_FLESH}"));
    let (out, code) = soma(&["holes", &path]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("2 holes of 7 signals"), "{out}");
    // `place` calls `ship`: `ship` first
    let (ship, place) = (out.find(". ship ").unwrap(), out.find(". place ").unwrap());
    assert!(ship < place, "{out}");
    assert!(out.contains("moves packed -> shipped"), "{out}");

    let (out, _) = soma(&["holes", &path, "--json"]);
    let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
    assert_eq!(v["holes"][1]["signal"], "Orders.place");
    assert_eq!(v["holes"][1]["after"], serde_json::json!(["Orders.ship"]));

    // the map shows the DECLARED frame of a hole, and marks it
    let (out, _) = soma(&["skeleton", &path]);
    assert!(out.contains("signal ship(id: String) -> String  moves packed -> shipped   // hole"), "{out}");
    assert!(out.contains("frames as declared"), "{out}");
}

#[test]
fn a_flesh_in_another_file_fills_the_skeleton() {
    let more = r#"
flesh Orders {
    on ship(id) {
        transition(id, "packed", "shipped")
        return "shipped"
    }
    on place(id, sku) {
        if take(sku, 1) {
            transition(id, "placed", "packed")
            log.set(id, sku)
            return ship(id)
        }
        return "no stock"
    }
}
"#;
    let path = project("files", &[
        ("app.cell", &format!("use lib::desk\nuse lib::orders\n{DESK_SKELETON}")),
        ("lib/desk.cell", DESK_FLESH),
        ("lib/orders.cell", more),
    ]);
    let (out, code) = soma(&["check", &path]);
    assert_eq!(code, 0, "{out}");
    assert!(!out.contains("hole"), "{out}");
    let (out, code) = soma(&["holes", &path]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("no holes — the 7 signals"), "{out}");
    let (out, code) = soma(&["verify", &path]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("VERIFY OK\n") && !out.contains("waits for a hole"), "{out}");
}

#[test]
fn a_flesh_that_does_not_fit_is_refused_with_the_fix() {
    let case = |name: &str, flesh: &str| soma(&["check", &program(name, &format!("{DESK_SKELETON}{flesh}"))]);

    let (out, code) = case("unknown", "flesh Order { on quote(n) { return n } }");
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("there is no `skeleton Order` — did you mean `flesh Orders`?"), "{out}");

    let (out, code) = case("twice", "flesh Orders { on quote(n) { return n } }\nflesh Orders { on quote(n) { return n + 1 } }");
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("`Orders.quote` has two bodies"), "{out}");

    let (out, code) = case("params", "flesh Orders { on quote(k) { return k } }");
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("does not match the skeleton's `signal quote(n: Int)` — write `on quote(n)`"), "{out}");

    let (out, code) = case("helper", "flesh Orders { on quote(n) { return _twice(n) }\n on _twice(k) { return k * 2 } }");
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("`_twice` is not a signal of `skeleton Orders`, so it is a private helper and its parameters need types"), "{out}");

    let (out, code) = soma(&["check", &program("plain", "cell A { on f() { return 1 } }\nflesh A { on g(n: Int) { return n } }")]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("`A` is a `cell`, not a `skeleton`"), "{out}");
}

#[test]
fn structure_and_bodies_cannot_trade_places() {
    // a body in the skeleton
    let (out, code) = soma(&["check", &program("body", "skeleton A {\n    face { signal f() -> Int }\n    on f() { return 1 }\n}")]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("a skeleton has no handler bodies") && out.contains("flesh A"), "{out}");
    // structure in the flesh
    let (out, code) = soma(&["check", &program("structure", "skeleton A {\n    face { signal f() -> Int }\n}\nflesh A {\n    memory { n: Map<String, Int> }\n    on f() { return 1 }\n}")]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("a flesh holds handler bodies only") && out.contains("skeleton A"), "{out}");
}

#[test]
fn the_skeleton_a_program_would_have_is_a_program() {
    // `soma skeleton app.cell > skeleton.cell`: every signal a hole, and it checks
    let path = program("derive_src", SHOP);
    let (derived, code) = soma(&["skeleton", &path]);
    assert_eq!(code, 0, "{derived}");
    let skeleton = program("derive_out", &derived);
    let (out, code) = soma(&["check", &skeleton]);
    assert_eq!(code, 0, "{out}\n{derived}");
    let (out, _) = soma(&["holes", &skeleton]);
    assert!(out.contains("7 holes of 7 signals"), "{out}");
    // callees first: `level` before `take`, `take` and `cancel` before `place`
    let (json, _) = soma(&["holes", &skeleton, "--json"]);
    let v: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
    let order: Vec<String> = v["holes"].as_array().unwrap().iter().map(|h| h["signal"].as_str().unwrap().to_string()).collect();
    let at = |n: &str| order.iter().position(|x| x == n).unwrap_or_else(|| panic!("{n} in {order:?}"));
    assert!(at("Stock.level") < at("Stock.take") && at("Stock.take") < at("Orders.place") && at("Orders.cancel") < at("Orders.place"), "{order:?}");
}

#[test]
fn words_of_the_frame_stay_ordinary_names() {
    // reads / writes / calls / moves / emits / skeleton / flesh are keywords
    // only where a frame or a top-level item can start
    let path = program("words", r#"
cell Words {
    memory { reads: Map<String, Int> }
    on count(calls: Int, moves: Int) {
        let skeleton = calls + moves
        let flesh = skeleton * 2
        reads.set("n", flesh)
        return reads.get("n")
    }
}
"#);
    let (out, code) = soma(&["run", &path, "count", "2", "3"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.trim().ends_with("10"), "{out}");
}

// ── phase 3: a flesh does what its frame says, and nothing else ──────

/// `check` on DESK_SKELETON with `flesh`, and one substitution in the skeleton.
fn desk(name: &str, edit: (&str, &str), flesh: &str) -> (String, i32) {
    assert!(edit.0.is_empty() || DESK_SKELETON.contains(edit.0), "{}", edit.0);
    let skeleton = if edit.0.is_empty() { DESK_SKELETON.to_string() } else { DESK_SKELETON.replacen(edit.0, edit.1, 1) };
    soma(&["check", &program(name, &format!("{skeleton}{DESK_FLESH}{flesh}"))])
}

const SHIP: &str = "flesh Orders {\n    on ship(id) {\n        transition(id, \"packed\", \"shipped\")\n        return \"shipped\"\n    }\n";

#[test]
fn a_body_inside_its_frame_passes() {
    let (out, code) = desk("c_ok", ("", ""), &format!("{SHIP}
    on place(id, sku) {{
        if take(sku, 1) {{
            transition(id, \"packed\")
            _note(id, sku)
            return ship(id)
        }}
        return \"no stock\"
    }}
    on _note(id: String, sku: String) {{ log.set(id, sku) }}
}}"));
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("All checks passed") && !out.contains("warning"), "{out}");
}

#[test]
fn a_write_a_read_a_call_an_emit_or_an_edge_outside_the_frame_is_refused() {
    // a write — through a helper: its effects are the signal's
    let (out, code) = desk("c_write2", ("            writes log\n", ""), &format!("{SHIP}    on place(id, sku) {{ transition(id, \"packed\")  if take(sku, 1) {{ return ship(id) }}  _stash(id, sku)  return \"x\" }}\n    on _stash(id: String, sku: String) {{ log.set(id, sku) }}\n}}"));
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("`Orders.place` writes `log`, outside its frame (no `writes`)") && out.contains("`writes log`"), "{out}");

    // a read — of a slot, and of the machine (a signal that MOVES the
    // machine may read it; `quote` moves nothing)
    let (out, code) = desk("c_read", ("", ""), "flesh Orders2 { }".replace("Orders2 { }", "Orders {\n    on ship(id) { transition(id, \"packed\", \"shipped\")  return \"{get_status(id)} {log.get(id)}\" }\n}").as_str());
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("`Orders.ship` reads `log`, outside its frame") && !out.contains("reads `flow`"), "{out}");
    let (out, code) = soma(&["check", &program("c_read2", &format!("{DESK_SKELETON}{}", DESK_FLESH.replace("return _triple(n)", "return len(get_status(\"o\")) + _triple(n)")))]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("`Orders.quote` reads `flow`, outside its frame (no `reads`)"), "{out}");

    // a call of another cell's signal, and of an effectful builtin
    let (out, code) = desk("c_call", ("", ""), &format!("{SHIP}    on place(id, sku) {{ transition(id, \"packed\")  log.set(id, sku)  if take(sku, 1) {{ return ship(id) }}  return \"{{level(sku)}} at {{now()}}\" }}\n}}"));
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("`Orders.place` calls `Stock.level`, outside its frame (`calls Stock.take, ship`)"), "{out}");
    assert!(out.contains("`Orders.place` calls `now`, outside its frame"), "{out}");

    // an emit
    let (out, code) = desk("c_emit", ("", ""), &format!("{SHIP}    on place(id, sku) {{ transition(id, \"packed\")  log.set(id, sku)  emit placed(id)  if take(sku, 1) {{ return ship(id) }}  return \"x\" }}\n}}"));
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("`Orders.place` emits `placed`, outside its frame (no `emits`)"), "{out}");

    // an edge: by target, and by the source the body names
    let (out, code) = desk("c_move", ("", ""), "flesh Orders {\n    on ship(id) { transition(id, \"cancelled\")  return \"x\" }\n}");
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("`Orders.ship` moves an instance to `cancelled`, outside its frame (`moves packed -> shipped`)"), "{out}");
    let (out, code) = desk("c_move2", ("signal ship(id: String) -> String       moves packed -> shipped", "signal ship(id: String) -> String       moves placed -> cancelled"),
        "flesh Orders {\n    on ship(id) { transition(id, \"placed\", \"packed\")  return \"x\" }\n}");
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("`Orders.ship` moves `placed -> packed`, outside its frame"), "{out}");

    // a target computed at run time needs `moves * -> *`
    let dynamic = "flesh Orders {\n    on ship(id) { let to = \"shipped\"  transition(id, to)  return to }\n}";
    let (out, code) = desk("c_dyn", ("", ""), dynamic);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("target computed at run time") && out.contains("`moves * -> *`"), "{out}");
    let (out, code) = desk("c_dyn_ok", ("signal ship(id: String) -> String       moves packed -> shipped", "signal ship(id: String) -> String       moves * -> *"), dynamic);
    assert_eq!(code, 0, "{out}");
}

#[test]
fn a_frame_must_make_sense_on_the_skeleton_alone() {
    // no flesh at all: these are errors of the design, found before any body
    let case = |name: &str, from: &str, to: &str| {
        assert!(DESK_SKELETON.contains(from), "{from}");
        soma(&["check", &program(name, &DESK_SKELETON.replacen(from, to, 1))])
    };
    let (out, code) = case("w_slot", "reads items", "reads itms");
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("`Stock.level`: `reads itms` names no memory slot or state machine of `Stock`") && out.contains("did you mean `items`?"), "{out}");

    let (out, code) = case("w_foreign", "writes log\n", "writes log, items\n");
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("`items` is a slot of `Stock`") && out.contains("calls Stock."), "{out}");

    let (out, code) = case("w_edge", "moves placed -> cancelled", "moves packed -> cancelled");
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("`moves packed -> cancelled` is not an edge of `Orders.flow`") && out.contains("`placed -> cancelled`"), "{out}");

    let (out, code) = case("w_call", "calls Stock.take, ship", "calls Stock.tke, ship");
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("`Stock` has no signal `tke`") && out.contains("did you mean `Stock.take`?"), "{out}");

    let (out, code) = case("w_bare", "calls Stock.take, ship", "calls take, ship");
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("`take` is a signal of `Stock`: write `calls Stock.take`"), "{out}");

    let (out, code) = case("w_machine", "writes items  calls level", "writes items  calls level  moves a -> b");
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("`Stock` has no state machine"), "{out}");
}

#[test]
fn what_no_frame_touches_is_reported() {
    // an edge no signal moves, a slot no signal writes: warnings on the
    // skeleton alone, not errors (a design in progress still checks)
    let (out, code) = desk("u_edge", ("signal cancel(id: String) -> String     moves placed -> cancelled", "signal cancel(id: String) -> String"), "");
    assert_eq!(code, 1, "the flesh of `cancel` now leaves its frame: {out}");
    let (out, code) = soma(&["check", &program("u_edge2", &DESK_SKELETON.replacen("     moves placed -> cancelled", "", 1))]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("no signal moves `placed -> cancelled` of `Orders.flow`"), "{out}");

    let (out, code) = soma(&["check", &program("u_slot", &DESK_SKELETON.replacen("            writes log\n", "", 1))]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("no signal of `skeleton Orders` reads or writes the slot `log`"), "{out}");
}

#[test]
fn a_frame_wider_than_its_body_is_a_warning() {
    let (out, code) = desk("wide", ("signal quote(n: Int) -> Int", "signal quote(n: Int) -> Int  reads log  calls now"), "");
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("`Orders.quote`: the frame says `reads log`, the body never reads it"), "{out}");
    assert!(out.contains("`Orders.quote`: the frame says `calls now`, the body never calls it"), "{out}");
    // a hole has no body to be wider than
    assert!(!out.contains("`Orders.place`: the frame says"), "{out}");
}

#[test]
fn helpers_are_private_and_schedules_only_call() {
    let path = program("priv2", &format!("{DESK_SKELETON}{DESK_FLESH}
cell Admin {{
    on poke(n: Int) {{ return _triple(n) }}
}}"));
    let (out, code) = soma(&["check", &path]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("`Admin.poke` calls `Orders._triple`, a private helper of `skeleton Orders`"), "{out}");

    let with_tick = DESK_SKELETON.replacen("    face {\n        signal place", "    every 5s { log.set(\"t\", \"x\") }\n    face {\n        signal place", 1);
    let (out, code) = soma(&["check", &program("sched", &with_tick)]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("`every 5s` in `skeleton Orders` does more than call signals (writes log)") && out.contains("every 5s { tick() }"), "{out}");
    let calls_only = DESK_SKELETON.replacen("    face {\n        signal place", "    every 5s { quote(1) }\n    face {\n        signal place", 1);
    let (out, code) = soma(&["check", &program("sched_ok", &format!("{calls_only}{DESK_FLESH}"))]);
    assert_eq!(code, 0, "{out}");
}

#[test]
fn split_turns_a_cell_into_a_skeleton_and_its_flesh() {
    let path = program("split_src", &format!("// the shop\n{SHOP}
cell test ShopTests {{
    rules {{
        assert quote(2) == 6
        assert take(\"w\", 1) == false
    }}
}}"));
    let (before, _) = soma(&["test", &path]);
    let (split, code) = soma(&["skeleton", &path, "--split"]);
    assert_eq!(code, 0, "{split}");
    // structure verbatim, frames added, bodies verbatim in the flesh
    assert!(split.starts_with("// the shop\n"), "{split}");
    assert!(split.contains("skeleton Orders {") && split.contains("flesh Orders {"), "{split}");
    assert!(split.contains("        signal take(sku: String, n: Int) -> Bool\n            writes items\n            calls level\n"), "{split}");
    assert!(split.contains("        placed -> packed { guard { n <= 10 } }"), "{split}");
    assert!(split.contains("        items.set(sku, have - n)"), "{split}");
    // an entry point outside the face joins it; a helper stays private
    assert!(split.contains("signal audit()") && split.contains("signal bump()") && split.contains("signal remember_order(id: String, sku: String)"), "{split}");
    assert!(split.contains("cell test ShopTests {"), "{split}");

    let out_path = program("split_out", &split);
    let (out, code) = soma(&["check", &out_path]);
    assert_eq!(code, 0, "{out}\n{split}");
    assert!(!out.contains("hole") && !out.contains("frame"), "{out}");
    let (after, _) = soma(&["test", &out_path]);
    let tally = |s: &str| s.lines().rev().find(|l| l.contains("passed")).unwrap_or("").to_string();
    assert_eq!(tally(&before), tally(&after), "{before}\n{after}");
    assert!(tally(&after).contains("2 passed"), "{after}");
}

// ── phase 4: freezing, constants, the starter ────────────────────────

#[test]
fn a_frozen_skeleton_cannot_change_in_passing() {
    let path = program("freeze", &format!("{DESK_SKELETON}{DESK_FLESH}"));
    let (out, code) = soma(&["skeleton", &path, "--freeze"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("2 skeletons: Stock, Orders"), "{out}");
    let lock = std::path::Path::new(&path).with_file_name("skeleton.lock");
    assert!(std::fs::read_to_string(&lock).unwrap().contains("== Orders sha256:"), "lock written");
    assert_eq!(soma(&["check", &path]).1, 0);

    // comments, layout and the flesh are free
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, text.replace("skeleton Orders {", "// the order desk\nskeleton   Orders {")
        .replace("return _triple(n)", "return _triple(n) + 0")).unwrap();
    let (out, code) = soma(&["check", &path]);
    assert_eq!(code, 0, "{out}");

    // the structure is not: check, run and test all refuse
    std::fs::write(&path, text.replace("\n        packed -> shipped\n", "\n        packed -> shipped\n        shipped -> placed\n")).unwrap();
    let (out, code) = soma(&["check", &path]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("skeleton `Orders` differs from skeleton.lock:") && out.contains("+ shipped -> placed") && out.contains("--amend"), "{out}");
    assert!(!out.contains("skeleton `Stock`"), "{out}");
    let (out, code) = soma(&["run", &path, "quote", "1"]);
    assert_ne!(code, 0, "{out}");
    let (out, code) = soma(&["skeleton", &path, "--freeze"]);
    assert_eq!(code, 1, "freeze does not overwrite: {out}");

    // on purpose: amend prints the difference and the program checks again
    let (out, code) = soma(&["skeleton", &path, "--amend"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("+ shipped -> placed") && out.contains("amended"), "{out}");
    let (out, code) = soma(&["check", &path]);
    assert_eq!(code, 0, "{out}");

    // a skeleton added or removed after the freeze
    std::fs::write(&path, format!("{}\nskeleton Extra {{ face {{ signal f() -> Int }} }}", std::fs::read_to_string(&path).unwrap())).unwrap();
    let (out, code) = soma(&["check", &path]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("skeleton `Extra` is not in skeleton.lock"), "{out}");
}

#[test]
fn a_flesh_reads_the_constants_of_its_skeleton() {
    // above the skeleton, and from an imported file
    let path = project("consts", &[
        ("app.cell", "use lib::body\n\nflesh Till {\n    on over(n) { return n > LIMIT }\n}\n\nskeleton Till {\n    const LIMIT = 500\n    memory {\n        cash: Map<String, Int>\n        invariant cash <= LIMIT\n    }\n    face {\n        signal over(n: Int) -> Bool\n        signal label() -> String\n        signal put(n: Int) -> Int  writes cash\n    }\n}\n"),
        ("lib/body.cell", "flesh Till {\n    on label() { return \"at most {LIMIT}\" }\n    on put(n) {\n        cash.set(\"k\", min(n, LIMIT))\n        return LIMIT\n    }\n}\n"),
    ]);
    let (out, code) = soma(&["check", &path]);
    assert_eq!(code, 0, "{out}");
    assert!(soma(&["run", &path, "over", "501"]).0.trim().ends_with("true"));
    assert!(soma(&["run", &path, "label"]).0.contains("at most 500"));
    assert!(soma(&["run", &path, "put", "900"]).0.trim().ends_with("500"));
}

#[test]
fn init_skeleton_gives_a_project_that_passes_the_loop_with_one_hole() {
    let root = std::env::temp_dir().join("soma_skeleton_init");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let run = |args: &[&str], dir: &std::path::Path| {
        let o = Command::new(env!("CARGO_BIN_EXE_soma")).args(args).current_dir(dir).output().unwrap();
        (format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)), o.status.code().unwrap_or(-1))
    };
    let (out, code) = run(&["init", "--skeleton", "demo"], &root);
    assert_eq!(code, 0, "{out}");
    let app = root.join("demo");
    assert!(app.join("lib/counter.cell").exists(), "{out}");
    let (out, code) = run(&["check", "app.cell"], &app);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("All checks passed") && out.contains("1 hole") && out.contains("Counter.reset"), "{out}");
    let (out, code) = run(&["verify", "app.cell"], &app);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("VERIFY OK (skeleton"), "{out}");
    let (out, code) = run(&["test", "app.cell"], &app);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("8 passed, 0 failed"), "{out}");
    let (out, _) = run(&["skeleton", "app.cell", "Counter.reset"], &app);
    assert!(out.contains("writes     counts: Map<String, Int> [persistent]") && out.contains("invariant counts >= 0"), "{out}");
    let (out, code) = run(&["fmt", "--check", "app.cell", "lib/counter.cell"], &app);
    assert_eq!(code, 0, "the starter is canonical: {out}");
    // filling the hole, inside its frame
    let flesh = app.join("lib/counter.cell");
    let text = std::fs::read_to_string(&flesh).unwrap();
    std::fs::write(&flesh, text.replacen("    on add(n) {", "    on reset() {\n        counts.set(\"n\", 0)\n        return 0\n    }\n\n    on add(n) {", 1)).unwrap();
    let (out, code) = run(&["check", "app.cell"], &app);
    assert_eq!(code, 0, "{out}");
    assert!(!out.contains("hole"), "{out}");
    let (out, _) = run(&["verify", "app.cell"], &app);
    assert!(out.contains("VERIFY OK\n"), "{out}");
}

#[test]
fn a_helper_is_private_by_its_name() {
    // a handler outside the face that the outside could call is refused
    let (out, code) = soma(&["check", &program("pub_helper", &format!("{DESK_SKELETON}{}", DESK_FLESH.replace("_triple", "triple")))]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("`triple` is not a signal of `skeleton Orders`") && out.contains("`on _triple(…)`") && out.contains("signal triple(…)"), "{out}");
    // split keeps every handler the outside could call callable: it joins the face
    let (split, _) = soma(&["skeleton", &program("pub_split", "cell A {\n    memory { m: Map<String, Int> }\n    on run(k: String) { return helper(k) + _inner(k) }\n    on helper(k: String) { return m.get(k) ?? 0 }\n    on _inner(k: String) { m.set(k, 1)  return 1 }\n}\n"), "--split"]);
    assert!(split.contains("signal run(k: String)\n            writes m\n            calls helper"), "{split}");
    assert!(split.contains("signal helper(k: String)\n            reads m"), "{split}");
    assert!(!split.contains("signal _inner"), "{split}");
    let (out, code) = soma(&["check", &program("pub_split_out", &split)]);
    assert_eq!(code, 0, "{out}\n{split}");
}

// ── review, step 1: effects `check` let through ──────────────────────
//
// The red-team template: `pure` has no clause, so ANY effect of its body
// must be refused. Each body below was accepted and its effect showed in
// `SOMA_EFFECT_TRACE` (fixtures/skeleton_review/redteam).

fn bank(pure_and_more: &str, face: &str, extra: &str) -> String {
    format!(r#"{extra}
skeleton Bank {{
    memory {{
        acct: Map<String, Int> [persistent]
        aux: Map<String, Int> [persistent]
    }}
    state order {{
        initial: new
        new -> paid
        paid -> shipped
        new -> shipped
    }}
    face {{
        signal peek(id: String) -> Int      reads acct
        signal bump(id: String) -> Int      writes acct
        signal pay(id: String) -> String    moves new -> paid
        signal status(id: String) -> String reads order
        signal pure(id: String) -> Int
{face}
    }}
}}

flesh Bank {{
    on peek(id) {{ return acct.get(id) ?? 0 }}
    on bump(id) {{ acct.set(id, (acct.get(id) ?? 0) + 1) return 1 }}
    on pay(id) {{ transition(id, "new", "paid") return "paid" }}
    on status(id) {{ return get_status(id) ?? "new" }}
{pure_and_more}
}}
"#)
}

const W: &str = "    on _w(id: String) { acct.set(id, 99) return 1 }\n";

fn refused(name: &str, source: &str, needle: &str) {
    let (out, code) = soma(&["check", &program(name, source)]);
    assert_eq!(code, 1, "{name} must be refused:\n{out}");
    assert!(out.contains(needle), "{name}: expected {needle:?} in\n{out}");
}

#[test]
fn hole_is_not_an_annotation_a_body_can_wear() {
    // `[hole]` on a handler with a body switched conformance off while the body ran
    refused("r_hole_prop", &bank("    on pure(id) [hole] { acct.set(id, 99) return 1 }", "", ""), "unknown handler annotation [hole]");
    refused("r_hole_trans", &bank("    on pure(id) [hole] { transition(id, \"paid\") return 1 }", "", ""), "unknown handler annotation [hole]");
    // and on a plain cell too
    refused("r_hole_cell", "cell A { on f() [hole] { return 1 } }", "unknown handler annotation [hole]");
}

#[test]
fn a_local_that_may_not_be_bound_hides_nothing() {
    let cases: &[(&str, String, &str)] = &[
        // a `let` in a branch that does not run
        ("dead_branch", format!("    on pure(id) {{ if id == \"zzz\" {{ let _w = 1 }} return _w(id) }}\n{W}"), "writes `acct`"),
        ("dead_signal", "    on pure(id) { if id == \"zzz\" { let bump = 1 } return bump(id) }\n".to_string(), "calls `bump`"),
        ("dead_builtin", "    on pure(id) { if id == \"zzz\" { let write_file = 1 } write_file(\"/tmp/soma_x\", id) return 1 }\n".to_string(), "write_file"),
        // in a loop that does not run, after the call, bound by a lambda or a match elsewhere
        ("while_local", format!("    on pure(id) {{ while false {{ let _w = 1 }} return _w(id) }}\n{W}"), "writes `acct`"),
        ("later_let", format!("    on pure(id) {{ let r = _w(id) let _w = 3 return r }}\n{W}"), "writes `acct`"),
        ("lambda_param", format!("    on pure(id) {{ let xs = map(list(1), _w => 0) return _w(id) }}\n{W}"), "writes `acct`"),
        ("for_var", format!("    on pure(id) {{ for _w in list() {{ }} return _w(id) }}\n{W}"), "writes `acct`"),
        ("match_binder", format!("    on pure(id) {{ let r = match 5 {{ _w -> 0 }} return _w(id) }}\n{W}"), "writes `acct`"),
        // the slot itself, behind a local that is not there
        ("dead_slot", "    on pure(id) { if id == \"zzz\" { let acct = map() } acct.set(id, 1) return 1 }\n".to_string(), "writes `acct`"),
        ("later_slot", "    on pure(id) { let n = acct.get(id) ?? 0 let acct = map() return n }\n".to_string(), "reads `acct`"),
    ];
    for (name, body, needle) in cases {
        refused(&format!("r_scope_{name}"), &bank(body, "", ""), needle);
    }
    // another cell, behind a local of its name in a dead branch
    refused("r_scope_cell", &bank("    on pure(id) { if id == \"zzz\" { let Other = 1 } return Other.poke(id) }\n", "", "cell Other { on poke(id: String) { return 1 } }\n"), "calls `Other.poke`");
}

#[test]
fn a_local_that_is_certainly_bound_is_not_the_slot() {
    // false positives the flat scope gave: these bodies touch no memory
    let ok = |name: &str, body: &str| {
        let (out, code) = soma(&["check", &program(name, &bank(body, "", ""))]);
        assert_eq!(code, 0, "{name}:\n{out}");
        assert!(!out.contains("outside its frame"), "{name}:\n{out}");
    };
    ok("r_fp_lambda_param", "    on pure(id) { let xs = map(list(1, 2), aux => aux + 1) return len(xs) }\n");
    ok("r_fp_let", "    on pure(id) { let aux = map(\"a\", 1) return aux.get(\"a\") ?? 0 }\n");
    ok("r_fp_for", "    on pure(id) { let t = 0 for aux in list(1, 2) { t = t + aux } return t }\n");
    ok("r_fp_match", "    on pure(id) { return match 5 { aux -> aux + 1 } }\n");
    ok("r_fp_lambda_block", "    on pure(id) { let f = (aux, y) => aux + y return f(1, 2) }\n");
    // …and bound means bound from there on, in that block: before it, it is the slot
    refused("r_fp_before", &bank("    on pure(id) { let n = len(aux) let aux = 1 return n + aux }\n", "", ""), "reads `aux`");
}

#[test]
fn an_interior_cell_cannot_take_the_name_of_another_cell() {
    // the frames of `skeleton Bank` were read off the interior `cell Bank`
    refused("r_interior", &bank("    on pure(id) { acct.set(id, 99) return 1 }", "", "cell Outer {\n    interior {\n        cell Bank {\n            face { signal pure(id: String) -> Int }\n            on pure(id: String) { return 1 }\n        }\n    }\n    on run() { return 1 }\n}\n"), "interior cell 'Bank' shares its name with a skeleton");
    // two plain interior cells of one name stay legal (their tables are qualified by the path)
    let (out, code) = soma(&["check", &program("r_interior_plain", "cell W { on peek() { return 1 } }\ncell Sys {\n    interior {\n        cell W {\n            face { signal go(n: Int) }\n            on go(n: Int) { print(n) }\n        }\n    }\n    runtime { signal go(1) }\n}\n")]);
    assert_eq!(code, 0, "{out}");
}

#[test]
fn an_emit_cannot_reach_a_private_helper() {
    // declared or not, from the cell or from another one
    refused("r_emit_own", &bank(&format!("    on pure(id) {{ return 1 }}\n    on pure2(id) {{ emit _w(id) return 1 }}\n{W}"), "        signal pure2(id: String) -> Int  emits _w", ""), "`_w` is a private helper of `skeleton Bank`");
    refused("r_emit_other", &bank(&format!("    on pure(id) {{ return 1 }}\n    on aux1(id) {{ return _w(id) }}\n{W}"), "        signal aux1(id: String) -> Int  writes acct", "cell Other { on poke(id: String) { emit _w(id) return 1 } }\n"), "`Other.poke` emits `_w`");
    // a signal that listens answers for itself: the emitter names the event, no more
    let (out, code) = soma(&["check", &program("r_emit_ok", &bank("    on pure(id) { return 1 }\n    on pure2(id) { emit bump(id) return 1 }\n", "        signal pure2(id: String) -> Int  emits bump", ""))]);
    assert_eq!(code, 0, "{out}");
}

#[test]
fn a_transition_without_a_source_needs_every_edge_into_its_target() {
    // `shipped` is entered from `paid` and from `new`
    let body = "    on pure(id) { return 1 }\n    on ship(id) { transition(id, \"shipped\") return 1 }\n";
    refused("r_moves_partial", &bank(body, "        signal ship(id: String) -> Int  moves paid -> shipped", ""), "so it also takes `new -> shipped`");
    for (name, face, body) in [
        ("all", "        signal ship(id: String) -> Int  moves paid -> shipped, new -> shipped", body),
        ("star", "        signal ship(id: String) -> Int  moves * -> shipped", body),
        ("named", "        signal ship(id: String) -> Int  moves paid -> shipped", "    on pure(id) { return 1 }\n    on ship(id) { transition(id, \"paid\", \"shipped\") return 1 }\n"),
    ] {
        let (out, code) = soma(&["check", &program(&format!("r_moves_{name}"), &bank(body, face, ""))]);
        assert_eq!(code, 0, "{name}:\n{out}");
    }
}

#[test]
fn calls_in_every_form_are_calls() {
    // method form, pipes, interpolation in a `require` message, std
    let cases: &[(&str, String, &str)] = &[
        ("ufcs_helper", format!("    on pure(id) {{ return id._w() }}\n{W}"), "writes `acct`"),
        ("ufcs_signal", "    on pure(id) { return id.bump() }\n".to_string(), "calls `bump`"),
        ("method_transition", "    on pure(id) { id.transition(\"paid\") return 1 }\n".to_string(), "outside its frame"),
        ("pipe", format!("    on pure(id) {{ return id |> _w() }}\n{W}"), "writes `acct`"),
        ("require_msg", "    on pure(id) { require id == \"zzz\" else Bad \"x {bump(id)}\" return 1 }\n".to_string(), "calls `bump`"),
        ("nested_interp", "    on pure(id) { let r = map(list(id), x => \"v{bump(x)}\") return 1 }\n".to_string(), "calls `bump`"),
        ("std", "    on pure(id) { let r = std.transition(id, \"paid\") return 1 }\n".to_string(), "outside its frame"),
    ];
    for (name, body, needle) in cases {
        refused(&format!("r_call_{name}"), &bank(body, "", ""), needle);
    }
}

/// Every reproducer the review left: where `check` passes, nothing the
/// program does under test may fall outside a frame.
#[test]
fn the_review_reproducers_no_longer_leak() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf();
    let fixtures = repo.join("compiler/tests/fixtures/skeleton_review");
    let mut files: Vec<String> = Vec::new();
    for sub in ["redteam", "pl"] {
        for entry in walk(&fixtures.join(sub)) {
            if entry.extension().map_or(false, |e| e == "cell") { files.push(entry.display().to_string()); }
        }
    }
    assert!(files.len() > 80, "the fixtures are missing: {}", files.len());
    let Ok(out) = Command::new("python3").arg(repo.join("tools/frame_oracle.py")).arg("--soma").arg(env!("CARGO_BIN_EXE_soma")).args(&files).output() else { return };
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success() && !text.contains("MISSED"), "{text}");
}

fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else { return out };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() { out.extend(walk(&p)); } else { out.push(p); }
    }
    out
}

// ── review, step 2: `verify` reads a hole as its frame ───────────────

const TWO_MACHINES: &str = r#"
skeleton Approval {
    state review {
        initial: pending
        pending -> signed_off
    }
    face {
        signal sign(id: String) -> String   moves pending -> signed_off
    }
}
skeleton Pay {
    state payment {
        initial: due
        due -> paid
    }
    face {
        signal pay(id: String) -> String   moves due -> paid
    }
}
flesh Approval {
    on sign(id) { transition(id, "pending", "signed_off")  return "ok" }
}
"#;

#[test]
fn a_property_a_hole_can_break_is_open_not_proven() {
    let toml = "[verify.before.\"Pay.paid\"]\nrequires = [\"Approval.signed_off\"]\n";
    // `pay` is a hole: its frame lets it take due -> paid at any time
    let path = project("v_before_hole", &[("app.cell", TWO_MACHINES), ("soma.toml", toml)]);
    let (out, code) = soma(&["verify", &path]);
    assert_eq!(code, 0, "{out}");
    assert!(!out.contains("✓ before Pay.paid"), "a ✓ on a hole: {out}");
    assert!(out.contains("⚠ open obligation — before Pay.paid requires Approval.signed_off"), "{out}");
    // the conforming body that breaks it: the same line is now a ✗
    let filled = format!("{TWO_MACHINES}flesh Pay {{\n    on pay(id) {{ transition(id, \"due\", \"paid\")  return \"paid\" }}\n}}\n");
    let path = project("v_before_filled", &[("app.cell", &filled), ("soma.toml", toml)]);
    let (out, code) = soma(&["verify", &path]);
    assert!(out.contains("✗ before Pay.paid requires Approval.signed_off") && out.contains("VERIFY FAILED"), "{code} {out}");
    // and the body that respects it: proven
    let guarded = format!("{TWO_MACHINES}flesh Pay {{\n    on pay(id) {{\n        require Approval.reviewed(id) else NotSigned\n        transition(id, \"due\", \"paid\")\n        return \"paid\"\n    }}\n}}\n")
        .replace("signal sign(id: String) -> String   moves pending -> signed_off", "signal sign(id: String) -> String   moves pending -> signed_off\n        signal reviewed(id: String) -> Bool  reads review")
        .replace("on sign(id) {", "on reviewed(id) { return get_status(id) == \"signed_off\" }\n    on sign(id) {")
        .replace("signal pay(id: String) -> String   moves due -> paid", "signal pay(id: String) -> String   moves due -> paid  calls Approval.reviewed");
    let path = project("v_before_guarded", &[("app.cell", &guarded), ("soma.toml", toml)]);
    let (out, code) = soma(&["check", &path]);
    assert_eq!(code, 0, "{out}");
}

#[test]
fn an_invariant_a_hole_may_write_is_not_proven() {
    let src = "skeleton Bank {\n    memory {\n        balance: Map<String, Int>\n        invariant balance >= 0\n    }\n    face {\n        signal deposit(who: String, n: Int) -> Int\n            writes balance\n        signal get(who: String) -> Int  reads balance\n    }\n}\nflesh Bank {\n    on get(who) { return balance.get(who) ?? 0 }\n}\n";
    let (out, _) = soma(&["verify", &program("v_inv_hole", src)]);
    assert!(!out.contains("✓ invariant balance >= 0"), "{out}");
    assert!(out.contains("⚠ invariant balance >= 0") && out.contains("deposit → balance"), "{out}");
    // no frame writes the slot: no conforming flesh can either — that ✓ stands
    let (out, _) = soma(&["verify", &program("v_inv_none", &src.replace("            writes balance\n", ""))]);
    assert!(out.contains("✓ invariant balance >= 0"), "{out}");
}

#[test]
fn what_is_judged_on_bodies_waits_for_the_bodies() {
    let path = program("v_wait", &format!("{DESK_SKELETON}{DESK_FLESH}"));
    let (out, code) = soma(&["verify", &path]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("· termination: 2 holes — judged when they have a body"), "{out}");
    assert!(out.contains("is a hole — its frame allows {placed → packed}"), "{out}");
    assert!(!out.contains("✓ refinement: handler `place`") && !out.contains("✓ think-isolated"), "{out}");
    assert!(out.contains("✓ refinement: handler `cancel`"), "a written body is still judged: {out}");
}

#[test]
fn every_tick_of_a_skeleton_stays_a_tick_of_the_program() {
    // tools/skeleton_monotone.py on the demo apps: each ✓ of the derived
    // skeleton (all holes) must be a ✓ of the program, which is one
    // conforming flesh among all
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf();
    let apps: Vec<String> = ["treasury", "poker", "delivery", "hft", "airlock", "elevator", "exchange", "insulin", "therac", "loan-origination", "incident-response", "car-rental", "hiring", "ballot", "allocator", "trader", "rebalancer"]
        .iter().map(|a| repo.join(a).join("app.cell").display().to_string()).filter(|p| std::path::Path::new(p).exists()).collect();
    assert!(apps.len() >= 10, "{}", apps.len());
    let Ok(out) = Command::new("python3").arg(repo.join("tools/skeleton_monotone.py")).arg("--soma").arg(env!("CARGO_BIN_EXE_soma")).args(&apps).output() else { return };
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success() && !text.contains("BROKEN"), "{text}");
}

#[test]
fn a_guard_the_face_already_requires_is_proven() {
    let src = "cell G {\n    face {\n        signal go(id: String, cost: Int) -> String\n            requires cost > 0\n    }\n    state s {\n        initial: a\n        a -> b { guard { cost > 0 } }\n    }\n    on go(id: String, cost: Int) {\n        transition(id, \"a\", \"b\")\n        return \"b\"\n    }\n}\n";
    let (out, _) = soma(&["verify", &program("v_guard_requires", src)]);
    assert!(out.contains("✓ guard `cost > 0` on a -> b: proven"), "{out}");
}

// ── review, step 3: the lock covers the whole structure ──────────────

fn frozen(name: &str, files: &[(&str, &str)]) -> String {
    let path = project(name, files);
    let (out, code) = soma(&["skeleton", &path, "--freeze"]);
    assert_eq!(code, 0, "{out}");
    path
}

fn rewrite(path: &str, f: impl Fn(String) -> String) {
    std::fs::write(path, f(std::fs::read_to_string(path).unwrap())).unwrap();
}

const DESK_ALL: &str = "skeleton Desk {\n    memory { titles: Map<String, String> }\n    state ticket {\n        initial: new\n        new -> assigned\n        assigned -> closed\n    }\n    face {\n        signal assign(id: String) -> String   moves new -> assigned\n        signal close(id: String) -> String    moves assigned -> closed\n        signal status(id: String) -> String   reads ticket\n    }\n}\nflesh Desk {\n    on assign(id) { transition(id, \"new\", \"assigned\")  return \"a\" }\n    on close(id) { transition(id, \"assigned\", \"closed\")  return \"c\" }\n    on status(id) { return get_status(id) ?? \"new\" }\n}\n";

#[test]
fn a_cell_added_after_the_freeze_cannot_move_a_frozen_machine() {
    let path = frozen("l_rogue", &[("app.cell", DESK_ALL)]);
    // a plain cell without a machine moves the program's only one
    rewrite(&path, |s| format!("{s}cell Rogue {{\n    on nuke(id: String) {{ transition(id, \"assigned\")  transition(id, \"closed\")  return \"nuked\" }}\n}}\n"));
    let (out, code) = soma(&["check", &path]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("skeleton `Desk` differs from skeleton.lock") && out.contains("moved from outside by Rogue.nuke"), "{out}");
    // a cell that only CALLS its signals changes nothing: that is what a face is for
    rewrite(&path, |s| s.replace("transition(id, \"assigned\")  transition(id, \"closed\")", "assign(id)  close(id)"));
    let (out, code) = soma(&["check", &path]);
    assert_eq!(code, 0, "{out}");
}

#[test]
fn an_interior_skeleton_is_frozen_too() {
    let src = "cell Outer {\n    interior {\n        skeleton Inner {\n            memory { n: Map<String, Int> }\n            face {\n                signal get(k: String) -> Int   reads n\n            }\n        }\n    }\n    on run() { return 1 }\n}\nflesh Inner {\n    on get(k) { return n.get(k) ?? 0 }\n}\n";
    let path = frozen("l_interior", &[("app.cell", src)]);
    rewrite(&path, |s| s.replace("signal get(k: String) -> Int   reads n", "signal get(k: String) -> Int   writes n"));
    let (out, code) = soma(&["check", &path]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("skeleton `Inner` differs from skeleton.lock"), "{out}");
}

#[test]
fn the_lock_holds_under_another_file_name_and_for_every_command() {
    let path = frozen("l_names", &[("app.cell", DESK_ALL), ("cases.jsonl", "{\"input\": [\"t1\"], \"expected\": \"new\"}\n")]);
    let dir = std::path::Path::new(&path).parent().unwrap().to_path_buf();
    // the same program, changed, under another name
    let copy = dir.join("app2.cell");
    std::fs::write(&copy, DESK_ALL.replace("        assigned -> closed\n", "        assigned -> closed\n        new -> closed\n")).unwrap();
    let (out, code) = soma(&["check", &copy.display().to_string()]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("skeleton `Desk` differs from skeleton.lock") && out.contains("+ new -> closed"), "{out}");
    // …and the program itself, changed: eval, build and serve --no-check refuse like check does
    rewrite(&path, |s| s.replace("        assigned -> closed\n", "        assigned -> closed\n        new -> closed\n"));
    let cases = dir.join("cases.jsonl").display().to_string();
    for args in [vec!["eval", path.as_str(), "--handler", "status", "--dataset", cases.as_str()], vec!["build", path.as_str(), "-o", "/dev/null"], vec!["serve", path.as_str(), "--no-check", "-p", "0"], vec!["run", path.as_str(), "status", "t1"], vec!["test", path.as_str()], vec!["verify", path.as_str()]] {
        let (out, code) = soma(&args);
        // (eval runs `soma test` on a copy and reports that it does not check)
        assert!(out.contains("skeleton.lock") || (args[0] == "eval" && out.contains("fails `soma check`")), "{args:?}: {code} {out}");
        assert!(!out.contains("\"new\"\n") || code != 0, "{args:?} ran the changed program: {out}");
        assert_ne!(code, 0, "{args:?}: {out}");
    }
}

#[test]
fn a_lock_that_cannot_be_read_is_an_error_and_freeze_never_writes_over_one() {
    let path = frozen("l_corrupt", &[("app.cell", DESK_ALL)]);
    let lock = std::path::Path::new(&path).with_file_name("skeleton.lock");
    let good = std::fs::read_to_string(&lock).unwrap();
    assert!(good.contains("checksum sha256:"), "{good}");
    for (what, damaged) in [("cut", good[..good.len() / 2].to_string()), ("edited", good.replace("new -> assigned", "new -> closed")), ("garbage", "not a lock\n".to_string())] {
        std::fs::write(&lock, &damaged).unwrap();
        let (out, code) = soma(&["check", &path]);
        assert_eq!(code, 1, "{what}: {out}");
        assert!(out.contains("skeleton.lock") && out.contains("--amend") && (out.contains("checksum") || out.contains("cut short")), "{what}: {out}");
        let (out, code) = soma(&["skeleton", &path, "--freeze"]);
        assert_eq!(code, 1, "{what}: --freeze wrote over a lock: {out}");
        assert_eq!(std::fs::read_to_string(&lock).unwrap(), damaged, "{what}");
    }
    let (out, code) = soma(&["skeleton", &path, "--amend"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("replacing"), "{out}");
    assert_eq!(soma(&["check", &path]).1, 0);
}

#[test]
fn the_difference_names_the_signal_even_for_a_clause_another_signal_has() {
    let src = "skeleton A {\n    memory { m: Map<String, Int> [persistent] }\n    face {\n        signal f(x: Int) -> Int\n            ensures result >= 0\n            calls g\n        signal g() -> Int                reads m\n        signal h(x: Int) -> Int\n            ensures result >= 0\n            writes m\n    }\n}\n";
    let path = frozen("l_diff", &[("app.cell", src)]);
    // `f` gains `writes m` — a line `h` already has
    rewrite(&path, |s| s.replacen("            calls g\n", "            writes m\n            calls g\n", 1));
    let (out, code) = soma(&["check", &path]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("- signal f(x: Int) -> Int  ensures result >= 0  calls g"), "{out}");
    assert!(out.contains("+ signal f(x: Int) -> Int  ensures result >= 0  writes m  calls g"), "{out}");
    assert!(out.contains("(in `A`: signal f)"), "{out}");
}

#[test]
fn the_theorems_of_soma_toml_are_part_of_what_is_frozen() {
    let toml = "[package]\nname = \"x\"\n\n# the theorem\n[verify.before.\"Pay.paid\"]\nrequires = [\"Approval.signed_off\"]\n";
    let path = frozen("l_toml", &[("app.cell", TWO_MACHINES), ("soma.toml", toml)]);
    let toml_path = std::path::Path::new(&path).with_file_name("soma.toml");
    // a comment or another table: free
    std::fs::write(&toml_path, toml.replace("# the theorem", "# still the theorem").replace("name = \"x\"", "name = \"y\"")).unwrap();
    assert_eq!(soma(&["check", &path]).1, 0);
    // the property dropped: refused
    std::fs::write(&toml_path, "[package]\nname = \"x\"\n").unwrap();
    let (out, code) = soma(&["check", &path]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("`[verify]` tables of soma.toml"), "{out}");
}

// ── review, step 4: a brief that is enough ───────────────────────────

const INTENT: &str = r#"
cell type Receipt {
    variants {
        Paid { amount: Int }
        Refused { reason: String }
    }
}

// Till: the one place cash is counted.
skeleton Till {
    const LIMIT = 500
    memory {
        cash: Map<String, Int> [persistent]        // drawer -> cents (absent: 0)
        invariant cash >= 0
    }
    state drawer {
        initial: closed
        closed -> open              // the cashier signs in
        open -> closed
    }
    face {
        // Take `n` cents into `drawer`. Refused above LIMIT, or when the drawer is not open.
        // Paid carries the drawer's new total.
        signal take(drawer: String, n: Int) -> Receipt
            requires n >= 1
            reads drawer
            writes cash
            calls now, stamp
        signal stamp(t: Int) -> String          // "t=<t>"
    }
}
flesh Till {
    on stamp(t) { return "t={t}" }
}
"#;

#[test]
fn the_brief_carries_what_the_skeleton_says_in_words() {
    let path = program("b_intent", INTENT);
    let (out, code) = soma(&["skeleton", &path, "Till.take"]);
    assert_eq!(code, 0, "{out}");
    // the cell's and the signal's comments: what it is for, what the answer means
    assert!(out.contains("// Till: the one place cash is counted."), "{out}");
    assert!(out.contains("    // Take `n` cents into `drawer`. Refused above LIMIT, or when the drawer is not open.\n    // Paid carries the drawer's new total."), "{out}");
    // a slot's comment, the cell's constants, the types the signature names
    assert!(out.contains("cash: Map<String, Int> [persistent]   // drawer -> cents (absent: 0)"), "{out}");
    assert!(out.contains("const      LIMIT = 500"), "{out}");
    assert!(out.contains("types      Receipt = Paid { amount: Int } | Refused { reason: String }"), "{out}");
    // the machine it reads, whole, with what an id is and what a fresh one answers
    assert!(out.contains("machine    drawer — one instance per id") && out.contains("get_status(id) is () until the id's first transition (edition 2026: `closed`) — write `get_status(id) ?? \"closed\"`"), "{out}");
    assert!(out.contains("closed -> open   // the cashier signs in") && out.contains("open -> closed"), "{out}");
    // a builtin with its signature, a signal with its comment
    assert!(out.contains("now() -> Int"), "{out}");
    assert!(out.contains("stamp(t: Int) -> String") && out.contains("// \"t=<t>\""), "{out}");
    assert!(out.contains("flesh Till { on take(drawer, n) { … } }"), "{out}");

    let (out, _) = soma(&["skeleton", &path, "Till.take", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
    assert_eq!(v["consts"], serde_json::json!(["LIMIT = 500"]));
    assert_eq!(v["machines"][0]["name"], "drawer");
    assert!(v["intent"][0].as_str().unwrap().starts_with("Take `n` cents"), "{v}");
}

#[test]
fn a_hole_whose_intent_is_not_written_down_says_so() {
    let (out, _) = soma(&["skeleton", &program("b_nointent", &INTENT.replace("        // Take `n` cents into `drawer`. Refused above LIMIT, or when the drawer is not open.\n        // Paid carries the drawer's new total.\n", "")), "Till.take"]);
    assert!(out.contains("no comment on this signal in the skeleton"), "{out}");
}

#[test]
fn holes_are_grouped_by_cell_and_can_be_narrowed() {
    let path = program("b_holes", DESK_SKELETON);
    let (out, code) = soma(&["holes", &path]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("\nStock (2)\n") && out.contains("\nOrders (5)\n"), "{out}");
    let (out, code) = soma(&["holes", &path, "Stock"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("7 holes of 7 signals") && out.contains("Stock (2)") && !out.contains("Orders ("), "{out}");
    let (out, code) = soma(&["holes", &path, "Nope"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("names no cell"), "{out}");
}

#[test]
fn verify_summary_prints_what_needs_attention() {
    let path = program("b_summary", &format!("{DESK_SKELETON}{DESK_FLESH}"));
    let (full, _) = soma(&["verify", &path]);
    let (out, code) = soma(&["verify", &path, "--summary"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.lines().count() < full.lines().count() / 2, "{out}");
    assert!(!out.contains('✓') || out.contains("proven (✓)"), "{out}");
    assert!(out.contains("proven (✓)") && out.contains("VERIFY OK (skeleton") && out.contains("2 holes"), "{out}");
    assert!(out.lines().filter(|l| l.trim_start().starts_with('⚠')).count() == full.lines().filter(|l| l.trim_start().starts_with('⚠')).count(), "{out}\n{full}");
}

#[test]
fn init_skeleton_here() {
    let root = std::env::temp_dir().join("soma_skeleton_init_here");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let o = Command::new(env!("CARGO_BIN_EXE_soma")).args(["init", "--skeleton", "."]).current_dir(&root).output().unwrap();
    assert!(o.status.success(), "{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    assert!(root.join("app.cell").exists() && root.join("lib/counter.cell").exists());
}

#[test]
fn two_cells_may_each_have_a_private_helper_of_one_name() {
    // "write Other._key(...) if you meant Other's" proposed a call no cell can make
    let src = "skeleton A {\n    face { signal f(k: String) -> String }\n}\nskeleton B {\n    face { signal g(k: String) -> String }\n}\nflesh A {\n    on f(k) { return _key(k) }\n    on _key(k: String) { return \"a:\" + k }\n}\nflesh B {\n    on g(k) { return _key(k) }\n    on _key(k: String) { return \"b:\" + k }\n}\n";
    let path = program("b_helpers", src);
    let (out, code) = soma(&["check", &path]);
    assert_eq!(code, 0, "{out}");
    assert!(!out.contains("if you meant"), "{out}");
    assert!(soma(&["run", &path, "B.g", "x"]).0.contains("b:x"));
}

// ── red team, round 2 ────────────────────────────────────────────────

#[test]
fn r2_a_bound_local_still_falls_back_nowhere() {
    // L1: `let acct = 1  acct.set(id, 5)` wrote the SLOT at run time (the
    // method call fell back to storage) while check read `acct` as the local.
    // The run no longer falls back: a local of that name is the local.
    let path = program("r2_l1", &format!("{}\ncell test T {{ rules {{ assert_fails pure(\"k\")  assert peek(\"k\") == 0 }} }}\n", bank("    on pure(id) { let acct = 1  let r = acct.set(id, 5)  return 1 }", "", "")));
    let (out, _) = soma(&["test", &path]);
    assert!(out.contains("2 passed, 0 failed"), "{out}");
    for body in ["    on pure(id) { for acct in list(1) { let r = acct.set(id, 5) }  return 1 }", "    on pure(id) { let acct = 1  let t = \"{acct.get(id)}\"  return 1 }"] {
        let path = program("r2_l1b", &format!("{}\ncell test T {{ rules {{ assert_fails pure(\"k\")  assert peek(\"k\") == 0 }} }}\n", bank(body, "", "")));
        let (out, _) = soma(&["test", &path]);
        assert!(out.contains("2 passed, 0 failed"), "{body}: {out}");
    }
}

#[test]
fn r2_a_local_named_like_a_cell_does_not_hide_a_method_form_call() {
    // L2: `let Other = id  Other.bump()` is bump(Other)
    refused("r2_l2", &bank("    on pure(id) { let Other = id  return Other.bump() }", "", "cell Other { on poke(id: String) { return 1 } }\n"), "calls `bump`");
    refused("r2_l2s", &bank("    on pure(id) { let Other = id  Other.bump()  return 1 }", "", "cell Other { on poke(id: String) { return 1 } }\n"), "calls `bump`");
}

const TWO_S: &str = "skeleton A {\n    memory { s: Map<String, Int> [persistent] }\n    face {\n        signal go(id: String) -> Int      writes s  calls B.apply, mk\n        signal mk() -> Any               writes s\n        signal peek(id: String) -> Int    reads s\n    }\n}\nskeleton B {\n    memory { s: Map<String, Int> [persistent] }\n    face {\n        signal apply(f: Any) -> Int\n        signal peek(id: String) -> Int    reads s\n    }\n}\nflesh B {\n    on apply(f) { let r = f(\"k\")  return 1 }\n    on peek(id) { return s.get(id) ?? 0 }\n    on _h(x: String) { s.set(x, 9)  return 1 }\n}\n";

#[test]
fn r2_a_lambda_that_leaves_its_handler_is_pure() {
    // L3 / L4: a lambda runs in the cell of whoever calls it. One that
    // touches memory or calls a handler may not be passed to a signal or returned.
    let a = |go: &str, mk: &str| format!("{TWO_S}flesh A {{\n    on go(id) {{ {go} }}\n    on mk() {{ {mk} }}\n    on peek(id) {{ return s.get(id) ?? 0 }}\n    on _h(x: String) {{ return 1 }}\n}}\n");
    refused("r2_l3a", &a("return B.apply(x => s.set(x, 7))", "return 1"), "an impure lambda");
    refused("r2_l3b", &a("return B.apply(x => _h(x))", "return 1"), "an impure lambda");
    refused("r2_l3c", &a("let f = x => s.set(x, 7)  return B.apply(f)", "return 1"), "an impure lambda");
    refused("r2_l4", &a("return 1", "return x => s.set(x, 7)"), "an impure lambda");
    // a pure lambda travels; an effectful one stays with builtins
    let (out, code) = soma(&["check", &program("r2_l3ok", &a("let n = [1, 2].map(x => s.set(\"{x}\", x))  return B.apply(x => len(x))", "return x => x"))]);
    assert_eq!(code, 0, "{out}");
}

#[test]
fn r2_a_transition_forgets_what_was_known_of_other_names() {
    // L5: `let j = id … transition(j, "a", "b")  transition(id, "x")` took b -> x
    let src = "skeleton Q {\n    state fl {\n        initial: a\n        a -> b\n        a -> x\n        b -> x\n    }\n    face {\n        signal st(id: String) -> String   reads fl\n        signal mv(id: String) -> Int      moves a -> b, a -> x\n    }\n}\nflesh Q {\n    on st(id) { return get_status(id) ?? \"a\" }\n    on mv(id) {\n        let j = id\n        if get_status(id) != \"a\" { return 0 }\n        transition(j, \"a\", \"b\")\n        transition(id, \"x\")\n        return 1\n    }\n}\n";
    refused("r2_l5", src, "so it also takes `b -> x`");
}

#[test]
fn r2_a_cost_bound_is_not_proven_over_a_hole() {
    // V1
    let src = "skeleton agent Boss {\n    cost { tokens: 500 }\n    state job { initial: idle  idle -> done }\n    face {\n        signal run(q: String) -> String  calls think  moves idle -> done\n    }\n}\n";
    let (out, code) = soma(&["check", &program("r2_v1", src)]);
    assert_eq!(code, 0, "{out}");
    assert!(!out.contains("bound proven"), "{out}");
    assert!(out.contains("cost") && out.contains("hole"), "{out}");
}

#[test]
fn r2_the_verify_tables_are_read_as_toml() {
    // K1: a dotted key, or the tables moved inside a multi-line string
    let toml0 = "[package]\nname = \"x\"\n\n[verify.before.\"Pay.paid\"]\nrequires = [\"Approval.signed_off\"]\n";
    for (name, changed) in [
        ("dotted", "verify.cells = [\"Nope\"]\n[package]\nname = \"x\"\n\n[verify.before.\"Pay.paid\"]\nrequires = [\"Approval.signed_off\"]\n"),
        ("string", "[package]\nname = \"x\"\ndescription = \"\"\"\n[verify.before.\"Pay.paid\"]\nrequires = [\"Approval.signed_off\"]\n[x]\"\"\"\n"),
    ] {
        let path = frozen(&format!("r2_k1_{name}"), &[("app.cell", TWO_MACHINES), ("soma.toml", toml0)]);
        std::fs::write(std::path::Path::new(&path).with_file_name("soma.toml"), changed).unwrap();
        let (out, code) = soma(&["check", &path]);
        assert_eq!(code, 1, "{name}: {out}");
        assert!(out.contains("`[verify]` tables of soma.toml"), "{name}: {out}");
    }
    // the same tables written another way are the same tables
    let path = frozen("r2_k1_same", &[("app.cell", TWO_MACHINES), ("soma.toml", toml0)]);
    std::fs::write(std::path::Path::new(&path).with_file_name("soma.toml"), "[package]\nname = \"x\"\n[verify]\nbefore.\"Pay.paid\".requires = [ \"Approval.signed_off\" ]   # same\n").unwrap();
    let (out, code) = soma(&["check", &path]);
    assert_eq!(code, 0, "{out}");
}

#[test]
fn r2_a_frozen_skeleton_is_held_wherever_the_entry_file_is() {
    // K2: the lock beside app.cell, the skeleton in lib/, another entry file in lib/
    let lib = DESK_ALL;
    let path = frozen("r2_k2", &[("app.cell", "use lib::desk\ncell Main { on run() { return status(\"t\") } }\n"), ("lib/desk.cell", lib), ("lib/main.cell", "use desk\ncell Main2 { on run() { return status(\"t\") } }\n")]);
    let dir = std::path::Path::new(&path).parent().unwrap().to_path_buf();
    std::fs::write(dir.join("lib/desk.cell"), lib.replace("        assigned -> closed\n", "        assigned -> closed\n        new -> closed\n")).unwrap();
    assert_eq!(soma(&["check", &path]).1, 1);
    let o = Command::new(env!("CARGO_BIN_EXE_soma")).args(["check", "main.cell"]).current_dir(dir.join("lib")).output().unwrap();
    let out = format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    assert!(!o.status.success() && out.contains("skeleton `Desk` differs from"), "{out}");
    // …and through a directory of symbolic links
    #[cfg(unix)]
    {
        let other = dir.join("elsewhere");
        std::fs::create_dir_all(&other).unwrap();
        std::os::unix::fs::symlink(dir.join("app.cell"), other.join("app.cell")).unwrap();
        std::os::unix::fs::symlink(dir.join("lib"), other.join("lib")).unwrap();
        let o = Command::new(env!("CARGO_BIN_EXE_soma")).args(["check", "app.cell"]).current_dir(&other).output().unwrap();
        let out = format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
        assert!(!o.status.success() && out.contains("skeleton `Desk` differs from"), "{out}");
    }
}

#[test]
fn r2_the_types_a_frozen_signature_names_are_frozen() {
    // K3
    let src = "cell type Order { variants { Order { amount: Int } } }\ncell type Verdict { variants { Paid  Refused } }\nskeleton Pay {\n    face { signal pay(o: Order) -> Verdict }\n}\n";
    let path = frozen("r2_k3", &[("app.cell", src)]);
    rewrite(&path, |s| s.replace("Paid  Refused", "Paid  Refused  Pending"));
    let (out, code) = soma(&["check", &path]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("skeleton `Pay` differs from skeleton.lock") && out.contains("Pending"), "{out}");
}

#[test]
fn r2_a_status_test_is_not_a_named_source() {
    // FP1 (round 2) asked that `if (get_status(id) ?? "placed") != "placed" { return }` establish
    // the source. Round 3 showed ten ways to fool any such inference: it is withdrawn. The body
    // names the edge — `transition(id, "placed", "cancelled")` — which the run enforces.
    let sk = "skeleton Orders {\n    state flow {\n        initial: placed\n        placed -> packed\n        placed -> cancelled\n        packed -> cancelled\n    }\n    face { signal cancel(id: String) -> String  moves placed -> cancelled }\n}\n";
    refused("r2_fp1_guarded", &format!("{sk}flesh Orders {{\n    on cancel(id) {{ if (get_status(id) ?? \"placed\") != \"placed\" {{ return \"too late\" }}  transition(id, \"cancelled\")  return \"ok\" }}\n}}\n"), "Name the source");
    let (out, code) = soma(&["check", &program("r2_fp1_named", &format!("{sk}flesh Orders {{\n    on cancel(id) {{ if (get_status(id) ?? \"placed\") != \"placed\" {{ return \"too late\" }}  transition(id, \"placed\", \"cancelled\")  return \"ok\" }}\n}}\n"))]);
    assert_eq!(code, 0, "{out}");
}

#[test]
fn r2_reads_of_a_machine_the_frame_also_moves_is_not_called_unused() {
    // FP2
    let src = "skeleton M {\n    state a { initial: x  x -> y }\n    face { signal step(id: String) -> String  reads a  moves x -> y }\n}\nflesh M {\n    on step(id) { if (get_status(id) ?? \"x\") == \"x\" { transition(id, \"x\", \"y\") }  return \"y\" }\n}\n";
    let (out, code) = soma(&["check", &program("r2_fp2", src)]);
    assert_eq!(code, 0, "{out}");
    assert!(!out.contains("never reads it"), "{out}");
}

#[test]
fn r2_a_frame_cannot_move_a_machine_its_cell_cannot_reach() {
    // FP3: a cell with its own machine names another cell's
    let src = "skeleton Approval {\n    state sign { initial: waiting  waiting -> signed_off }\n    face { signal sign_it(id: String) -> Int  moves waiting -> signed_off }\n}\nskeleton Pay {\n    state pay { initial: due  due -> paid }\n    face {\n        signal both(id: String) -> Int  moves Approval.sign: waiting -> signed_off  moves due -> paid\n    }\n}\n";
    refused("r2_fp3", src, "cannot move `Approval.sign`");
}

#[test]
fn r2_the_round_two_reproducers_no_longer_leak() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf();
    let files: Vec<String> = walk(&repo.join("compiler/tests/fixtures/skeleton_review/round2")).into_iter()
        .filter(|p| p.extension().map_or(false, |e| e == "cell") && p.to_string_lossy().contains("/L")).map(|p| p.display().to_string()).collect();
    assert!(files.len() >= 5, "{files:?}");
    let Ok(out) = Command::new("python3").arg(repo.join("tools/frame_oracle.py")).arg("--soma").arg(env!("CARGO_BIN_EXE_soma")).args(&files).output() else { return };
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success() && !text.contains("MISSED"), "{text}");
}

#[test]
fn the_one_page_card_says_only_what_runs() {
    // tests/fixtures/flesh_card/app.cell uses every construct `soma docs flesh` shows
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/flesh_card");
    let work = std::env::temp_dir().join("soma_skeleton_card");
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).unwrap();
    for f in ["app.cell", "soma.toml"] { std::fs::copy(dir.join(f), work.join(f)).unwrap(); }
    let path = work.join("app.cell").display().to_string();
    let (out, code) = soma(&["check", &path]);
    assert_eq!(code, 0, "{out}");
    let (out, _) = soma(&["test", &path]);
    assert!(out.contains("11 passed, 0 failed"), "{out}");
    let (card, code) = soma(&["docs", "flesh"]);
    assert_eq!(code, 0);
    assert!(card.lines().count() < 120, "the card is one page: {} lines", card.lines().count());
    for needle in ["emit low_stock(sku, n)", "Lot { sku: sku, qty: 3 }", "Refused(why) ->", "stock.entries.filter(e => e.value > 0)", "xs.sort_by(x => x.qty)", "substring(s, from, to)", "1 || 2 -> \"small\"", "idiv(a, b)", "get_status(id) ?? \"open\"", "require n >= 1 else BadAmount", "r.kind, r.detail", "stock.has(sku)", "xs.push(v)"] {
        assert!(card.contains(needle), "{needle}");
    }
}

// ── red team, round 3 ────────────────────────────────────────────────

#[test]
fn r3_an_impure_lambda_is_written_where_it_runs() {
    // A syntactic "does it escape" rule lost to every indirection (a list, an
    // alias, an assignment, an if-expression, a curried lambda, a helper's
    // answer). The rule is now positional: a lambda that touches memory,
    // a handler or a machine may only be the direct argument of a builtin
    // that runs it at once (`xs.map(x => …)`).
    let a = |go: &str| format!("{TWO_S}flesh A {{\n    on go(id) {{ {go} }}\n    on mk() {{ return 1 }}\n    on peek(id) {{ return s.get(id) ?? 0 }}\n    on _h(x: String) {{ return 1 }}\n}}\n");
    for (name, go) in [
        ("list", "let fs = [x => s.set(x, 7)]  return B.apply(fs[0])"),
        ("alias", "let f = x => s.set(x, 7)  let g = f  return B.apply(g)"),
        ("assigned", "let f = 0  f = x => s.set(x, 7)  return B.apply(f)"),
        ("if_expr", "let f = if id == \"a\" { x => s.set(x, 7) } else { x => 1 }  return B.apply(f)"),
        ("curried", "let f = a => b => s.set(b, 7)  let g = f(1)  return B.apply(g)"),
        ("index_set", "return B.apply(x => { s[x] = 7  1 })"),
        ("coalesce", "let f = () ?? (x => s.set(x, 7))  return B.apply(f)"),
        ("map_field", "let m = map(\"f\", x => s.set(x, 7))  return B.apply(m.f)"),
    ] {
        refused(&format!("r3_lam_{name}"), &a(go), "lambda");
    }
    // where it runs: fine. Pure: goes anywhere, and may capture a local named like a slot
    for (name, go) in [
        ("inline", "let n = [\"a\", \"b\"].map(x => s.set(x, 7))  return 1"),
        ("method", "let n = [\"a\"].filter(x => (s.get(x) ?? 0) > 0)  return len(n)"),
        ("pure_bound", "let f = x => len(x)  return B.apply(f)"),
        ("captures_local", "let acct = len(id) * 2  return B.apply(x => acct + len(x))"),
    ] {
        let (out, code) = soma(&["check", &program(&format!("r3_lam_ok_{name}"), &a(go))]);
        assert_eq!(code, 0, "{name}: {out}");
    }
}

#[test]
fn r3_only_a_named_source_pins_an_edge() {
    // The body's own reasoning about where an instance is (`if get_status(id)
    // == "a" …`) was tracked by the TEXT of the id and of status locals: ten
    // ways to fool it. A frame is now satisfied by `transition(id, from, to)`
    // — checked at run time too — or by listing every edge into the target.
    let sk = "skeleton Q {\n    state fl {\n        initial: a\n        a -> b\n        a -> x\n        b -> x\n    }\n    face {\n        signal st(id: String) -> String   reads fl\n        signal ab(id: String) -> Int      moves a -> b\n        signal mv(id: String) -> Int      moves a -> x  calls ab\n    }\n}\nflesh Q {\n    on st(id) { return get_status(id) ?? \"a\" }\n    on ab(id) { transition(id, \"a\", \"b\")  return 1 }\n";
    for (name, mv) in [
        ("rebound", "if (get_status(id) ?? \"a\") != \"a\" { return 0 }  let id = \"other\"  transition(id, \"x\")  return 1"),
        ("stale_alias", "let s = get_status(id) ?? \"a\"  let r = ab(id)  if s == \"a\" { transition(id, \"x\") }  return 1"),
        ("same_condition", "if (get_status(id) ?? \"a\") == \"a\" && ab(id) == 1 { transition(id, \"x\") }  return 1"),
        ("match", "match get_status(id) ?? \"a\" { \"a\" -> transition(id, \"x\")  _ -> 0 }  return 1"),
        ("guarded", "if (get_status(id) ?? \"a\") != \"a\" { return 0 }  transition(id, \"x\")  return 1"),
    ] {
        refused(&format!("r3_src_{name}"), &format!("{sk}    on mv(id) {{ {mv} }}\n}}\n"), "without saying from where");
    }
    let (out, code) = soma(&["check", &program("r3_src_named", &format!("{sk}    on mv(id) {{ if (get_status(id) ?? \"a\") != \"a\" {{ return 0 }}  transition(id, \"a\", \"x\")  return 1 }}\n}}\n"))]);
    assert_eq!(code, 0, "{out}");
}

#[test]
fn r3_std_is_the_builtin_it_names() {
    // `std.now()` under `calls now`; `std.len(x)` is free; `std.think` is think
    let (out, code) = soma(&["check", &program("r3_std_ok", &bank("    on pure(id) { return std.len(id) }\n    on clock(id) { return std.now() }", "        signal clock(id: String) -> Int  calls now", ""))]);
    assert_eq!(code, 0, "{out}");
    refused("r3_std_now", &bank("    on pure(id) { return std.now() }", "", ""), "calls `now`");
    refused("r3_std_move", &bank("    on pure(id) { let r = std.transition(id, \"paid\")  return 1 }", "", ""), "outside its frame");
}

#[test]
fn r3_a_cost_bound_is_not_proven_over_a_hole_that_emits() {
    let src = "skeleton agent Boss {\n    cost { tokens: 500 }\n    state job { initial: idle  idle -> done }\n    face {\n        signal ask(q: String) -> String   calls think\n        signal run(q: String) -> String   emits ask\n        signal fin(q: String) -> String   moves idle -> done\n    }\n}\nflesh Boss {\n    on ask(q) { return think(\"a {q}\", map(\"max_tokens\", 100)) }\n    on fin(q) { transition(q, \"idle\", \"done\")  return \"ok\" }\n}\n";
    let (out, code) = soma(&["check", &program("r3_cost", src)]);
    assert_eq!(code, 0, "{out}");
    assert!(!out.contains("bound proven") && out.contains("not proven yet"), "{out}");
}

#[test]
fn r3_the_lock_follows_types_into_types_and_holds_the_edition() {
    let src = "cell type Item { variants { Item { sku: String, qty: Int } } }\ncell type Order { variants { Order { id: String, item: Item } } }\nskeleton Pay {\n    face { signal pay(o: Order) -> Int }\n}\n";
    let path = frozen("r3_k_nested", &[("app.cell", src), ("soma.toml", "[package]\nname = \"x\"\n")]);
    rewrite(&path, |s| s.replace("qty: Int", "qty: String, admin: Bool"));
    let (out, code) = soma(&["check", &path]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("skeleton `Pay` differs from skeleton.lock") && out.contains("admin: Bool"), "{out}");
    // the brief shows the nested type too
    let (brief, _) = soma(&["skeleton", &program("r3_k_nested_brief", src), "Pay.pay"]);
    assert!(brief.contains("Order = Order { id: String, item: Item }") && brief.contains("Item = Item {"), "{brief}");
    // the edition decides what a fresh `get_status` answers: it is part of what is frozen
    let path = frozen("r3_k_edition", &[("app.cell", src), ("soma.toml", "[package]\nname = \"x\"\n")]);
    std::fs::write(std::path::Path::new(&path).with_file_name("soma.toml"), "[package]\nname = \"x\"\nedition = \"2027\"\n").unwrap();
    let (out, code) = soma(&["check", &path]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("edition"), "{out}");
}

#[test]
fn r3_fix_does_not_say_all_is_well_under_a_broken_lock() {
    let path = frozen("r3_fix", &[("app.cell", DESK_ALL)]);
    rewrite(&path, |s| s.replace("        assigned -> closed\n", "        assigned -> closed\n        new -> closed\n"));
    let (out, code) = soma(&["fix", &path]);
    assert!(!out.contains("All checks passed") && out.contains("skeleton.lock"), "{code} {out}");
    assert_ne!(code, 0, "{out}");
}

#[test]
fn r3_the_brief_reads_block_comments_tools_and_listeners() {
    let src = "skeleton Stock {\n    memory { items: Map<String, Int> }\n    face {\n        /* Take `n` units of `sku`.\n           false when there are fewer. */\n        signal take(sku: String, n: Int) -> Bool  writes items  emits taken\n        // Told after every take.\n        signal taken(sku: String) -> Int\n        tool lookup(id: String) -> Map \"Look an item up\"  reads items\n    }\n}\n";
    let path = program("r3_brief", src);
    let (out, _) = soma(&["skeleton", &path, "Stock.take"]);
    assert!(out.contains("// Take `n` units of `sku`.") && out.contains("false when there are fewer."), "{out}");
    assert!(out.contains("emits      taken(sku: String) -> Int") && out.contains("Told after every take."), "{out}");
    let (out, _) = soma(&["skeleton", &path, "Stock.taken"]);
    assert!(out.contains("emitted by Stock.take"), "{out}");
    let (out, _) = soma(&["skeleton", &path, "Stock.lookup"]);
    assert!(out.contains("Look an item up"), "{out}");
}

#[test]
fn r3_the_round_three_reproducers_no_longer_leak() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf();
    let files: Vec<String> = walk(&repo.join("compiler/tests/fixtures/skeleton_review/round3")).into_iter()
        .filter(|p| p.file_name().map_or(false, |n| n == "app.cell") && p.to_string_lossy().contains("/A")).map(|p| p.display().to_string()).collect();
    assert!(files.len() >= 15, "{files:?}");
    let Ok(out) = Command::new("python3").arg(repo.join("tools/frame_oracle.py")).arg("--soma").arg(env!("CARGO_BIN_EXE_soma")).args(&files).output() else { return };
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success() && !text.contains("MISSED"), "{text}");
}

// ── red team, round 4 ────────────────────────────────────────────────

/// `A.go` hands a lambda that writes `s` to the pure `B.apply`, which calls it.
fn handed(go: &str, extra: &str) -> String {
    format!("skeleton A {{\n    memory {{ s: Map<String, Int> [persistent] }}\n    face {{\n        signal go(id: String) -> Int      writes s  calls B.apply, B.filter\n        signal peek(id: String) -> Int    reads s\n    }}\n}}\nskeleton B {{\n    memory {{ s: Map<String, Int> [persistent] }}\n    face {{\n        signal apply(f: Any) -> Int\n        signal filter(f: Any) -> Int\n        signal peek(id: String) -> Int    reads s\n    }}\n}}\nflesh B {{\n    on apply(f) {{ let r = f(\"j\")  return 1 }}\n    on filter(f) {{ let r = f(\"j\")  return 1 }}\n    on peek(id) {{ return s.get(id) ?? 0 }}\n}}\nflesh A {{\n    on go(id) {{ {go} }}\n    on peek(id) {{ return s.get(id) ?? 0 }}\n}}\n{extra}")
}

#[test]
fn r4_the_positional_rule_has_no_side_door() {
    for (name, go) in [
        // reduce's initial value is not the lambda it applies
        ("reduce_initial", "let g = reduce([], x => s.set(x, 7), p => p.acc)  return B.apply(g)"),
        // a binder somewhere in the lambda does not hide the slot everywhere in it
        ("dead_let", "let g = x => { if false { let s = 0 }  s.set(x, 7)  1 }  return B.apply(g)"),
        ("nested_param", "let g = x => { let t = [0].map(s => s)  s.set(x, 7)  1 }  return B.apply(g)"),
        // a signal named like a lambda builtin is a signal
        ("cell_filter", "return B.filter(x => s.set(x, 7))"),
        // "visibly a list" is a list literal, list(…) or range(…): nothing that may be a string
        ("map_concat", "let m = map(concat(\"k\", \"\"), x => s.set(x, 7))  return B.apply(m.get(\"k\"))"),
        ("map_keys_field", "let r = map(\"keys\", \"j\")  let m = map(r.keys, x => s.set(x, 7))  return B.apply(m.get(\"j\"))"),
    ] {
        refused(&format!("r4_lam_{name}"), &handed(go, ""), "lambda");
    }
    // plain `map(xs, f)`: refused, and the message says why and what to write
    let (out, code) = soma(&["check", &program("r4_lam_plain_map", &handed("let xs = [\"a\"]  let n = map(xs, x => s.get(x) ?? 0)  return len(n)", ""))]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("`map(a, f)` written as a plain call") && out.contains("xs.map(") , "{out}");
}

#[test]
fn r4_at_run_time_a_lambda_away_from_its_handler_can_do_nothing() {
    // Whatever `check` misses — and a plain cell or a test is not checked
    // at all — the run refuses: a lambda called outside the handler that
    // wrote it may not touch memory, call a handler, move a machine or emit.
    let tests = "cell test T {\n    rules {\n        assert_fails Plain.go(\"k\") matching \"lambda\"\n        assert B.peek(\"j\") == 0\n        assert_fails B.apply(x => B.peek(x)) matching \"lambda\"\n        assert B.apply(x => len(x)) == 1\n        assert Plain.local(\"k\") == 2\n    }\n}\n";
    let plain = "cell Plain {\n    memory { secret: Map<String, Int> }\n    on go(id: String) { return B.apply(x => secret.set(x, 7)) }\n    on local(id: String) { let f = x => secret.set(x, 2)  f(id)  return secret.get(id) ?? 0 }\n}\n";
    let path = program("r4_runtime", &handed("return 1", &format!("{plain}{tests}")));
    let (out, _) = soma(&["test", &path]);
    assert!(out.contains("5 passed, 0 failed"), "{out}");
}

#[test]
fn r4_verify_does_not_count_what_a_short_circuit_may_skip() {
    // `false && Approval.sign(id) == 1` never signs: the rule is not proven
    let toml = "[verify.before.\"Pay.paid\"]\nrequires = [\"Approval.signed_off\"]\n";
    let base = TWO_MACHINES.replace("signal pay(id: String) -> String   moves due -> paid", "signal pay(id: String) -> String   moves due -> paid  calls Approval.sign");
    for (name, cond) in [("and", "let ok = id == \"zzz\" && Approval.sign(id) == \"ok\""), ("or", "let ok = id != \"zzz\" || Approval.sign(id) == \"ok\""), ("coalesce", "let ok = id ?? Approval.sign(id)")] {
        let src = format!("{base}flesh Pay {{\n    on pay(id) {{ {cond}  transition(id, \"due\", \"paid\")  return \"paid\" }}\n}}\n");
        let path = project(&format!("r4_v_{name}"), &[("app.cell", &src), ("soma.toml", toml)]);
        let (out, _) = soma(&["verify", &path]);
        assert!(!out.contains("✓ before Pay.paid") && out.contains("✗ before Pay.paid"), "{name}: {out}");
    }
}

#[test]
fn r4_the_brief_and_holes_refuse_a_skeleton_that_left_its_lock() {
    let path = frozen("r4_lock_brief", &[("app.cell", DESK_SKELETON)]);
    rewrite(&path, |s| s.replacen("signal quote(n: Int) -> Int", "signal quote(n: Int) -> Int  writes log", 1));
    for args in [vec!["skeleton", path.as_str(), "Orders.quote"], vec!["holes", path.as_str()]] {
        let (out, code) = soma(&args);
        assert_eq!(code, 1, "{args:?}: {out}");
        assert!(out.contains("skeleton.lock") && out.contains("--amend"), "{args:?}: {out}");
    }
    // the map still prints (it is how one looks), with the warning
    let (out, code) = soma(&["skeleton", &path]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("skeleton.lock"), "{out}");
}

#[test]
fn r4_a_lock_is_found_however_deep_the_file_is_and_nested_locks_can_be_amended() {
    let deep = "lib/a/b/c/d/e/f/g/sk.cell";
    let sk = "skeleton S {\n    memory { s: Map<String, Int> }\n    face { signal get(k: String) -> Int  reads s }\n}\nflesh S { on get(k) { return s.get(k) ?? 0 } }\n";
    let path = frozen("r4_lock_deep", &[("app.cell", "use lib::a::b::c::d::e::f::g::sk\ncell Main { on run() { return get(\"k\") } }\n"), (deep, sk)]);
    let dir = std::path::Path::new(&path).parent().unwrap().to_path_buf();
    std::fs::write(dir.join(deep), sk.replace("reads s", "writes s")).unwrap();
    let (out, code) = soma(&["check", &dir.join(deep).display().to_string()]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("skeleton `S` differs from"), "{out}");

    // two locks, one above the other: a cell of the outer program that moves the
    // inner program's machine is the OUTER lock's business
    let inner = "skeleton M {\n    state fl { initial: a  a -> b }\n    face { signal go(id: String) -> Int  moves a -> b }\n}\nflesh M { on go(id) { transition(id, \"a\", \"b\")  return 1 } }\n";
    let path = project("r4_lock_two", &[("app.cell", "use sub::m\ncell Main { on run() { return 1 } }\n"), ("sub/m.cell", inner)]);
    let dir = std::path::Path::new(&path).parent().unwrap().to_path_buf();
    assert_eq!(soma(&["skeleton", &dir.join("sub/m.cell").display().to_string(), "--freeze"]).1, 0);
    assert_eq!(soma(&["skeleton", &path, "--freeze"]).1, 0);
    rewrite(&path, |s| format!("{s}cell Rogue {{ on nuke(id: String) {{ transition(id, \"b\")  return 1 }} }}\n"));
    let (out, code) = soma(&["check", &path]);
    assert_eq!(code, 1, "{out}");
    assert_eq!(out.matches("differs from").count(), 1, "one lock complains, the one that can be amended: {out}");
    assert_eq!(soma(&["skeleton", &path, "--amend"]).1, 0);
    let (out, code) = soma(&["check", &path]);
    assert_eq!(code, 0, "{out}");
}

#[test]
fn r4_a_builtin_called_in_method_form_is_the_builtin() {
    // another cell has a signal named `filter`: `xs.filter(f)` still runs the builtin
    let src = "skeleton Z {\n    face { signal filter(a: List, b: Any) -> Int }\n}\nflesh Z { on filter(a, b) { return 0 } }\nskeleton A {\n    memory { s: Map<String, Int> }\n    face { signal go(id: String) -> Int  reads s }\n}\nflesh A {\n    on go(id) { let xs = [\"a\", \"b\"]  let r = xs.filter(x => s.has(x))  return len(r) }\n}\n";
    let (out, code) = soma(&["check", &program("r4_method_builtin", src)]);
    assert_eq!(code, 0, "{out}");
}

// ── red team, round 5 ────────────────────────────────────────────────

#[test]
fn r5_a_lambda_away_from_its_cell_reads_nothing_and_opens_nothing() {
    // two fast paths (`len(slot)`, `listslot[i]`) and the socket builtins went round the guard
    let src = "skeleton B {\n    memory {\n        secret: Map<String, Int>\n        lst: List<Int>\n    }\n    face {\n        signal fill() -> Int  writes secret, lst\n        signal apply(f: Any) -> Any\n    }\n}\nflesh B {\n    on fill() { secret.set(\"a\", 1)  secret.set(\"b\", 2)  lst.push(41)  return 1 }\n    on apply(f) { return f(\"j\") }\n}\ncell test T {\n    rules {\n        assert fill() == 1\n        assert_fails apply(x => len(secret)) matching \"lambda\"\n        assert_fails apply(x => lst[0]) matching \"lambda\"\n        assert_fails apply(x => len(lst)) matching \"lambda\"\n        assert_fails apply(x => link(\"127.0.0.1:9\")) matching \"lambda\"\n        assert_fails apply(x => ws_connect(\"ws://127.0.0.1:9\")) matching \"lambda\"\n        assert apply(x => len(x)) == 1\n    }\n}\n";
    let (out, _) = soma(&["test", &program("r5_guard", src)]);
    assert!(out.contains("7 passed, 0 failed"), "{out}");
}

#[test]
fn r5_a_lambda_is_at_home_in_its_cell_whatever_carried_it_there() {
    // a pure combinator from another cell applies a callback written here: at home
    let comb = "cell Util { on comb() { return g => g(\"a\") } }\ncell A {\n    memory { s: Map<String, Int> }\n    on go() { s.set(\"a\", 5)  let c = Util.comb()  return c(k => s.get(k) ?? 0) }\n    on _each(xs: List, f: Any) { for x in xs { f(x) }  return 1 }\n    on fill() { _each([\"p\", \"q\"], x => s.set(x, 1))  return s.size }\n    on mk() { return x => s.get(x) ?? 0 }\n    on via() { s.set(\"z\", 9)  let f = mk()  return f(\"z\") }\n}\ncell test T {\n    rules {\n        assert go() == 5\n        assert fill() >= 2\n        assert via() == 9\n    }\n}\n";
    let (out, _) = soma(&["test", &program("r5_home", comb)]);
    assert!(out.contains("3 passed, 0 failed"), "{out}");
}

#[test]
fn r5_verify_proves_nothing_from_what_may_not_run_or_is_not_what_it_seems() {
    let toml = "[verify.before.\"Pay.paid\"]\nrequires = [\"Approval.signed_off\"]\n";
    let base = TWO_MACHINES
        .replace("signal sign(id: String) -> String   moves pending -> signed_off", "signal sign(id: String) -> String   moves pending -> signed_off\n        signal approved(id: String) -> Bool  reads review")
        .replace("on sign(id) {", "on approved(id) { return (get_status(id) ?? \"pending\") == \"signed_off\" }\n    on sign(id) {")
        .replace("signal pay(id: String) -> String   moves due -> paid", "signal pay(id: String) -> String   moves due -> paid  calls Approval.sign, Approval.approved");
    for (name, body) in [
        // a call inside an interpolation, in a place that may not run
        ("interp_require", "require id != \"\" else Bad \"{Approval.sign(id)}\""),
        ("interp_and", "let ok = id == \"zz\" && \"{Approval.sign(id)}\" == \"ok\""),
        ("interp_coalesce", "let r = 1 ?? \"{Approval.sign(id)}\""),
        // an id reassigned in a nested block is another id afterwards
        ("assign_nested", "let j = id  if id != \"zz\" { j = \"other\" }  Approval.sign(j)"),
        ("assign_in_for", "let j = id  for x in [1] { j = \"other\" }  Approval.sign(j)"),
        ("guard_assign", "let j = id  if id != \"zz\" { j = \"vip\" }  if !Approval.approved(j) { return \"no\" }"),
        // a match binder named like the id is not the id
        ("match_binder", "let r = match \"other\" { id -> Approval.sign(id) }"),
    ] {
        let src = format!("{base}flesh Pay {{\n    on pay(id) {{ {body}  transition(id, \"due\", \"paid\")  return \"paid\" }}\n}}\n");
        let path = project(&format!("r5_v_{name}"), &[("app.cell", &src), ("soma.toml", toml)]);
        let (out, _) = soma(&["verify", &path]);
        assert!(!out.contains("✓ before Pay.paid"), "{name}: {out}");
    }
    // …and what does prove it still does
    let src = format!("{base}flesh Pay {{\n    on pay(id) {{ require Approval.approved(id) else NotSigned  transition(id, \"due\", \"paid\")  return \"paid\" }}\n}}\n");
    let path = project("r5_v_proven", &[("app.cell", &src), ("soma.toml", toml)]);
    let (out, _) = soma(&["verify", &path]);
    assert!(out.contains("✓ before Pay.paid"), "{out}");
}

#[test]
fn r5_a_comment_that_leans_on_its_neighbour_brings_it_along() {
    let src = "skeleton L {\n    memory { cash: Map<String, Int>  pts: Map<String, Int> }\n    face {\n        // Pays `amount` in cash. More than is due: error kind `Overpaid`. Returns the due amount afterwards.\n        signal pay_cash(folio: String, amount: Int) -> Int  writes cash\n        // The same, paid with points.\n        signal pay_points(folio: String, amount: Int) -> Int  writes pts\n    }\n}\n";
    let (out, _) = soma(&["skeleton", &program("r5_same", src), "L.pay_points"]);
    assert!(out.contains("The same, paid with points.") && out.contains("pay_cash") && out.contains("error kind `Overpaid`"), "{out}");
}

#[test]
fn r5_an_id_never_moved_is_read_as_its_edition_reads_it() {
    // edition 2027: `get_status(id)` of an id never moved is `()`, so the
    // test below is false and `sign` does not run — the product read the
    // initial state there and proved the rule
    let base = TWO_MACHINES.replace("signal pay(id: String) -> String   moves due -> paid", "signal pay(id: String) -> String   moves due -> paid  reads payment  calls Approval.sign");
    let rule = "[verify.before.\"Pay.paid\"]\nrequires = [\"Approval.signed_off\"]\n";
    for (name, test, edition, proven) in [
        ("bare_2026", "get_status(id) == \"due\"", "", true),
        ("bare_2027", "get_status(id) == \"due\"", "[package]\nname = \"u\"\nedition = \"2027\"\n", false),
        ("initial_2027", "(get_status(id) ?? \"due\") == \"due\"", "[package]\nname = \"u\"\nedition = \"2027\"\n", true),
        ("other_2027", "(get_status(id) ?? \"paid\") == \"due\"", "[package]\nname = \"u\"\nedition = \"2027\"\n", false),
    ] {
        let src = format!("{base}flesh Pay {{\n    on pay(id) {{ if {test} {{ Approval.sign(id) }}  transition(id, \"due\", \"paid\")  return \"paid\" }}\n}}\n");
        let path = project(&format!("r5_unborn_{name}"), &[("app.cell", &src), ("soma.toml", &format!("{edition}{rule}"))]);
        let (out, _) = soma(&["verify", &path]);
        assert_eq!(out.contains("✓ before Pay.paid"), proven, "{name}: {out}");
    }
}

const R6_WALLET: &str = "skeleton W {\n    memory { bal: Map<String, Int> }\n    face {\n        signal put(k: String, n: Int) -> Int  writes bal\n        signal get(k: String) -> Int  reads bal\n    }\n}\nflesh W {\n    on put(k, n) { return _store(k, n) }\n    on _store(k: String, n: Int) { bal.set(k, n)  return n }\n    on get(k) { return bal.get(k) ?? 0 }\n}\n";

#[test]
fn r6_a_private_helper_of_a_skeleton_is_not_run_from_the_command_line() {
    // no frame speaks for `_store`: it runs under the signals that reach it
    let path = program("r6_run_helper", R6_WALLET);
    let (out, code) = soma(&["run", &path, "put", "a", "5"]);
    assert!(code == 0 && out.trim() == "5", "{out}");
    for args in [vec!["run", &path, "_store", "a", "5"], vec!["run", &path, "W._store", "a", "5"], vec!["run", &path, "--signal", "_store", "a", "5"]] {
        let (out, code) = soma(&args);
        assert!(code != 0 && out.contains("private helper"), "{args:?}: {out}");
    }
    // a plain cell's `_h` runs as before
    let plain = program("r6_run_plain", "cell P { on _h(n: Int) { return n + 1 } }\n");
    let (out, code) = soma(&["run", &plain, "_h", "1"]);
    assert!(code == 0 && out.trim() == "2", "{out}");
}

#[test]
fn r6_describe_and_lint_refuse_what_the_lock_refuses() {
    let path = program("r6_views_lock", R6_WALLET);
    let (out, code) = soma(&["skeleton", &path, "--freeze"]);
    assert_eq!(code, 0, "{out}");
    for cmd in ["describe", "lint"] {
        let (out, code) = soma(&[cmd, &path]);
        assert!(code == 0 && !out.contains("skeleton.lock"), "{cmd}: {out}");
    }
    let changed = R6_WALLET.replace("signal get(k: String) -> Int  reads bal", "signal get(k: String) -> Int  reads bal\n        signal wipe() -> Int  writes bal");
    std::fs::write(&path, changed).unwrap();
    for cmd in ["describe", "lint"] {
        let (out, code) = soma(&[cmd, &path]);
        assert!(code != 0 && out.contains("differs from skeleton.lock"), "{cmd}: {out}");
    }
}

#[test]
fn r6_a_record_returned_where_the_face_names_its_type_is_that_type() {
    // `return Rec { … }` was "a Map literal" where the face says `-> Rec`
    let src = "cell type Montants { variants { Montants { total: Int, patient: Int } } }\ncell F {\n    face { signal a(t: Int) -> Montants  signal n(t: Int) -> Int }\n    on a(t: Int) { return Montants { total: t, patient: t } }\n    on n(t: Int) { return t }\n}\ncell test T { rules { assert F.a(3).total == 3 } }\n";
    let path = program("r6_record_return", src);
    let (out, code) = soma(&["check", &path]);
    assert_eq!(code, 0, "{out}");
    let (out, _) = soma(&["test", &path]);
    assert!(out.contains("1 passed, 0 failed"), "{out}");
    // …and a record is still not an Int
    let bad = src.replace("on n(t: Int) { return t }", "on n(t: Int) { return Montants { total: t, patient: t } }");
    let (out, code) = soma(&["check", &program("r6_record_return_bad", &bad)]);
    assert!(code != 0 && out.contains("E0040"), "{out}");
}

#[test]
fn r6_a_status_read_through_the_other_cells_getter_is_evidence() {
    // `require Approval.status(id) == "signed_off"`: the form the docs give
    let toml = "[verify.before.\"Pay.paid\"]\nrequires = [\"Approval.signed_off\"]\n";
    let base = TWO_MACHINES
        .replace("signal sign(id: String) -> String   moves pending -> signed_off", "signal sign(id: String) -> String   moves pending -> signed_off\n        signal status(id: String) -> String  reads review")
        .replace("on sign(id) {", "on status(id) { return get_status(id) ?? \"pending\" }\n    on sign(id) {")
        .replace("signal pay(id: String) -> String   moves due -> paid", "signal pay(id: String, other: String) -> String   moves due -> paid  calls Approval.status");
    for (name, body, proven) in [
        ("getter", "require Approval.status(id) == \"signed_off\" else NotSigned", true),
        ("getter_if", "if Approval.status(id) != \"signed_off\" { return \"no\" }", true),
        ("getter_let", "let s = Approval.status(id)  require s == \"signed_off\" else NotSigned", true),
        // the status of ANOTHER id says nothing of this one
        ("other_id", "require Approval.status(other) == \"signed_off\" else NotSigned", false),
        ("let_then_changed", "let s = Approval.status(id)  s = \"signed_off\"  require s == \"signed_off\" else NotSigned", false),
    ] {
        let src = format!("{base}flesh Pay {{\n    on pay(id, other) {{ {body}  transition(id, \"due\", \"paid\")  return \"paid\" }}\n}}\n");
        let path = project(&format!("r6_getter_{name}"), &[("app.cell", &src), ("soma.toml", toml)]);
        let (out, _) = soma(&["verify", &path]);
        assert_eq!(out.contains("✓ before Pay.paid"), proven, "{name}: {out}");
    }
}

#[test]
fn r6_a_test_helper_calling_with_the_wrong_count_is_seen_by_check() {
    let src = "cell A {\n    on f(a: Int, b: Int) { return a + b }\n}\ncell test T {\n    on _d() { return A.f(1) }\n    rules {\n        assert _d() == 2\n    }\n}\n";
    let (out, code) = soma(&["check", &program("r6_helper_arity", src)]);
    assert!(code != 0 && out.contains("E0041") && out.contains("passes 1 argument"), "{out}");
    let (out, code) = soma(&["check", &program("r6_helper_arity_ok", &src.replace("A.f(1)", "A.f(1, 1)"))]);
    assert_eq!(code, 0, "{out}");
}

const R6_TWO_CELLS: &str = "// Money held per account.\nskeleton Wallets {\n    memory {\n        bal: Map<String, Int>   // account -> cents\n    }\n    face {\n        // Takes `n` cents from `k`. Not enough: error kind `Short`.\n        signal debit(k: String, n: Int) -> Int  writes bal\n        signal balance(k: String) -> Int  reads bal\n    }\n}\n// Orders, paid from a wallet.\nskeleton Orders {\n    memory { total: Map<String, Int> }\n    state order {\n        initial: open\n        open -> paid\n    }\n    face {\n        // Pays order `id` from the buyer's wallet.\n        signal pay(id: String, buyer: String) -> Int  reads total  moves open -> paid  calls Wallets.debit  emits order_paid\n        signal order_paid(id: String)\n    }\n}\n";

#[test]
fn r6_one_cell_of_a_large_map_is_shown_with_what_its_author_wrote() {
    let path = program("r6_cell_view", R6_TWO_CELLS);
    let (out, code) = soma(&["skeleton", &path, "Wallets"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("skeleton Wallets") && !out.contains("skeleton Orders"), "{out}");
    assert!(out.contains("// Money held per account.") && out.contains("// Takes `n` cents from `k`. Not enough: error kind `Short`.") && out.contains("account -> cents"), "{out}");
    assert!(out.contains("called by") && out.contains("Orders.pay"), "{out}");
    // a signal of that name, were there one, keeps its brief
    let (out, _) = soma(&["skeleton", &path, "Orders.pay"]);
    assert!(out.contains("flesh Orders { on pay(id, buyer)"), "{out}");
}

#[test]
fn r6_the_index_is_one_line_per_cell_and_says_who_talks_to_whom() {
    let path = program("r6_index", R6_TWO_CELLS);
    let (out, code) = soma(&["skeleton", &path, "--index"]);
    assert_eq!(code, 0, "{out}");
    let wallets = out.lines().find(|l| l.starts_with("Wallets")).unwrap_or_else(|| panic!("{out}"));
    let orders = out.lines().find(|l| l.starts_with("Orders")).unwrap_or_else(|| panic!("{out}"));
    assert!(wallets.contains("2 signals") && wallets.contains("2 holes") && wallets.contains("Money held per account"), "{out}");
    assert!(orders.contains("machine order (2 states)"), "{out}");
    assert!(out.contains("Orders -> Wallets") && out.contains("debit"), "{out}");
    assert!(out.lines().count() < 12, "{out}");
}

#[test]
fn r6_a_count_is_never_negative_and_verify_knows_it() {
    let src = "cell A {\n    memory {\n        fiches: Map<String, Int>\n        journal: List<String>\n        invariant fiches >= 0\n    }\n    face {\n        signal n() -> Int  ensures result >= 0\n        signal l() -> Int  ensures result >= 0\n        signal c() -> Int  ensures result >= 0\n        signal k() -> Int  ensures result >= 0\n        signal s() -> Int  ensures result >= 0\n        signal hidden() -> Int  ensures result >= 0\n        signal field() -> Int  ensures result >= 0\n    }\n    on n() { return fiches.size }\n    on l() { return journal.len }\n    on c() { return journal.count(x => x == \"a\") }\n    on k() { return fiches.keys.count(x => x == \"a\") }\n    on s() { return sum(fiches.values) }\n    on hidden() { let fiches = map(\"size\", -3)  return fiches.size }\n    on field() { let m = map(\"size\", -3)  return m.size }\n}\n";
    let (out, _) = soma(&["verify", &program("r6_counts", src)]);
    for h in ["n", "l", "c", "k", "s"] {
        assert!(out.lines().any(|l| l.contains('✓') && l.contains(&format!("{h}: ensures result >= 0"))), "{h}: {out}");
    }
    // a local of the slot's name (or any map) may hold a field `size`
    for h in ["hidden", "field"] {
        assert!(!out.lines().any(|l| l.contains('✓') && l.contains(&format!("{h}: ensures result >= 0"))), "{h}: {out}");
    }
}

#[test]
fn r6_the_summary_of_an_empty_skeleton_says_once_what_waits_and_which_hole_opens_a_rule() {
    let toml = "[verify.before.\"Pay.paid\"]\nrequires = [\"Approval.signed_off\"]\n";
    let src = TWO_MACHINES.replace("signal pay(id: String) -> String   moves due -> paid", "signal pay(id: String) -> String   moves due -> paid\n        signal count() -> Int  ensures result >= 0\n        signal size() -> Int  ensures result >= 0");
    let path = project("r6_summary_holes", &[("app.cell", &src), ("soma.toml", toml)]);
    let (out, _) = soma(&["verify", &path, "--summary"]);
    assert!(!out.contains("not proven for `?`"), "{out}");
    assert!(out.contains("2 contracts of holes wait for a body"), "{out}");
    // the rule a hole leaves open: which hole
    let open = out.lines().find(|l| l.contains("open obligation")).unwrap_or_else(|| panic!("{out}"));
    assert!(open.contains("Pay.pay"), "{out}");
}

#[test]
fn r6_verify_gives_no_tick_to_the_sixth_rounds_reproducers() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/skeleton_review/round6");
    for (dir, tick) in [
        ("V1_continue", "✓ before"), ("V2_std_transition", "✓ before"), ("V2b_std_transition_guard_proven", "✓ guard"),
        ("V2c_std_delegate_termination", "✓ termination"), ("V3a_interpolated_id", "✓ before"), ("V3b_interpolated_target", "✓ before"),
        ("V4_states_differing_by_case", "✓ before"), ("V5_guard_proven_effect_in_same_statement", "✓ guard"),
        ("V5b_guard_proven_effect_in_if_condition", "✓ guard"), ("V5c_guard_same_statement_list", "✓ guard"),
        ("V6_assuming_float", "✓ before"), ("V7_lambda_called_later", "✓ before"), ("V7b_lambda_handed_to_helper", "✓ before"),
    ] {
        let (out, _) = soma(&["verify", &format!("{root}/{dir}/app.cell")]);
        assert!(!out.contains(tick), "{dir}: {out}");
    }
}

#[test]
fn r6_the_frozen_soma_toml_holds_for_every_entry_file_of_its_directory() {
    // after a freeze on app.cell, soma.toml was changed: alt.cell, beside it, read the new one freely
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/skeleton_review/round6");
    for dir in ["K1_alt_entry_verify_tables", "K1b_alt_entry_edition"] {
        for entry in ["app.cell", "alt.cell"] {
            let (out, code) = soma(&["check", &format!("{root}/{dir}/{entry}")]);
            assert!(code != 0 && out.contains("skeleton.lock") && out.contains("soma.toml"), "{dir}/{entry}: {out}");
        }
    }
}

#[test]
fn r6_a_transition_that_names_another_source_does_not_owe_an_edges_guard() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/skeleton_review/round6");
    let (out, code) = soma(&["check", &format!("{root}/A2_guard_var_pinned_source/app.cell")]);
    assert_eq!(code, 0, "{out}");
    // …and the variant returned where the face names its type passes too
    let (out, code) = soma(&["check", &format!("{root}/A1_return_record_variant/app.cell")]);
    assert_eq!(code, 0, "{out}");
}

#[test]
fn r6_reading_another_cells_machine_is_refused_where_it_is_declared() {
    // `reads Bank.order` was accepted in the skeleton and no body could be written for it
    let src = "skeleton Bank {\n    state order { initial: open  open -> paid }\n    face { signal pay(id: String) -> String  moves open -> paid }\n}\nskeleton Ship {\n    state parcel { initial: packed  packed -> sent }\n    face { signal send(id: String) -> String  moves packed -> sent }\n}\nskeleton Desk {\n    face {\n        signal peek(id: String) -> String  reads Bank.order\n    }\n}\n";
    let (out, code) = soma(&["check", &program("r6_reads_foreign_machine", src)]);
    assert!(code != 0 && out.contains("cannot read `Bank.order`") && out.contains("calls Bank."), "{out}");
    // with one machine in the program and none here, it is readable: accepted
    let one = src.replace("skeleton Ship {\n    state parcel { initial: packed  packed -> sent }\n    face { signal send(id: String) -> String  moves packed -> sent }\n}\n", "");
    let (out, code) = soma(&["check", &program("r6_reads_only_machine", &one)]);
    assert_eq!(code, 0, "{out}");
}

const R7_VISIT: &str = "skeleton Visit {\n    state v {\n        initial: waiting\n        waiting -> open\n        open -> done\n    }\n    face {\n        signal open_visit(id: String) -> String     moves waiting -> open\n        signal close_visit(id: String) -> String    moves open -> done\n        signal require_open(id: String) -> Bool     reads v\n        signal require_seen(id: String) -> Bool     reads v\n    }\n}\nflesh Visit {\n    on open_visit(id) { transition(id, \"waiting\", \"open\")  return \"open\" }\n    on close_visit(id) { transition(id, \"open\", \"done\")  return \"done\" }\n    on require_open(id) { require get_status(id) == \"open\" else NotOpen  return true }\n    on require_seen(id) { require get_status(id) == \"open\" || get_status(id) == \"done\" else NotSeen  return true }\n}\nskeleton Desk {\n    state d {\n        initial: none\n        none -> requested\n    }\n    face {\n        signal request_lab(id: String) -> String   moves none -> requested   calls Visit.require_open, Visit.require_seen\n    }\n}\n";

#[test]
fn r7_a_rule_on_the_state_the_other_machine_is_in_now() {
    // `before … requires` is history ("was open once"); `while` is the present
    let toml = "[verify.before.\"Desk.requested\"]\nwhile = [\"Visit.open\"]\n";
    for (name, guard, proven) in [("strict", "Visit.require_open(id)", true), ("lax", "Visit.require_seen(id)", false), ("none", "true", false)] {
        let src = format!("{R7_VISIT}flesh Desk {{\n    on request_lab(id) {{\n        let ok = {guard}\n        transition(id, \"none\", \"requested\")\n        return \"requested\"\n    }}\n}}\n");
        let path = project(&format!("r7_while_{name}"), &[("app.cell", &src), ("soma.toml", toml)]);
        let (out, _) = soma(&["verify", &path]);
        assert_eq!(out.contains("✓ entering Desk.requested while Visit.open"), proven, "{name}: {out}");
        if !proven { assert!(out.contains("✗ entering Desk.requested while Visit.open"), "{name}: {out}"); }
    }
    // a hole is read as its frame: an open obligation, not a ✓
    let path = project("r7_while_hole", &[("app.cell", R7_VISIT), ("soma.toml", toml)]);
    let (out, _) = soma(&["verify", &path]);
    assert!(!out.contains("✓ entering") && out.contains("open obligation"), "{out}");
    // the history rule on the same program still holds for the lax guard
    let src = format!("{R7_VISIT}flesh Desk {{\n    on request_lab(id) {{\n        let ok = Visit.require_seen(id)\n        transition(id, \"none\", \"requested\")\n        return \"requested\"\n    }}\n}}\n");
    let path = project("r7_while_history", &[("app.cell", &src), ("soma.toml", "[verify.before.\"Desk.requested\"]\nrequires = [\"Visit.open\"]\n")]);
    let (out, _) = soma(&["verify", &path]);
    assert!(out.contains("✓ before Desk.requested requires Visit.open"), "{out}");
}

#[test]
fn r7_a_rule_before_an_initial_state_is_refused_not_ticked() {
    // every instance is in its initial state from the start: nothing precedes it
    let src = format!("{R7_VISIT}flesh Desk {{\n    on request_lab(id) {{ transition(id, \"none\", \"requested\")  return \"requested\" }}\n}}\n");
    for toml in ["[verify.before.\"Desk.none\"]\nrequires = [\"Visit.open\"]\n", "[verify.before.\"Desk.none\"]\nwhile = [\"Visit.open\"]\n"] {
        let path = project("r7_initial_target", &[("app.cell", &src), ("soma.toml", toml)]);
        let (out, code) = soma(&["verify", &path]);
        assert!(code != 0 && !out.contains("✓ before Desk.none") && !out.contains("✓ entering Desk.none") && out.contains("initial state"), "{out}");
    }
}

#[test]
fn r7_refs_says_where_a_signal_is_declared_written_called_and_tested() {
    let lib = "skeleton Wallets {\n    memory { bal: Map<String, Int> }\n    face {\n        signal debit(k: String, n: Int) -> Int  writes bal\n        signal balance(k: String) -> Int  reads bal\n    }\n}\nskeleton Orders {\n    face {\n        signal pay(id: String, buyer: String) -> Int  calls Wallets.debit\n    }\n}\n";
    let flesh = "flesh Wallets {\n    on debit(k, n) {\n        bal.set(k, (bal.get(k) ?? 0) - n)\n        return balance(k)\n    }\n    on balance(k) { return bal.get(k) ?? 0 }\n}\nflesh Orders {\n    on pay(id, buyer) {\n        let left = Wallets.debit(buyer, 5)\n        return left\n    }\n}\n";
    let test = "use app\ncell test T {\n    on _setup() { return Wallets.debit(\"a\", 1) }\n    rules {\n        assert Wallets.debit(\"a\", 0) == 0\n        let who = \"b\"\n        assert \"{Wallets.debit(who, 0)}\" == \"0\"\n    }\n}\n";
    let path = project("r7_refs", &[("app.cell", "use lib::s\nuse body::f\n"), ("lib/s.cell", lib), ("body/f.cell", flesh), ("test_wallets.cell", test)]);
    let (out, code) = soma(&["refs", &path, "Wallets.debit"]);
    assert_eq!(code, 0, "{out}");
    for needle in ["declared", "lib/s.cell:4", "body", "body/f.cell:2", "Orders.pay", "body/f.cell:10", "test_wallets.cell:3", "test_wallets.cell:5", "test_wallets.cell:7"] {
        assert!(out.contains(needle), "{needle}: {out}");
    }
    // a call of the cell's own signal, written bare
    let (out, _) = soma(&["refs", &path, "Wallets.balance"]);
    assert!(out.contains("Wallets.debit") && out.contains("body/f.cell:4"), "{out}");
    // the brief says where to edit
    let (out, _) = soma(&["skeleton", &path, "Wallets.debit"]);
    assert!(out.contains("lib/s.cell:4") && out.contains("body/f.cell:2"), "{out}");
    let (out, code) = soma(&["refs", &path, "Wallets.nope"]);
    assert!(code != 0 && out.contains("no signal"), "{out}");
}

#[test]
fn r7_checking_a_directory_goes_through_the_entry_not_through_each_part() {
    // `soma check .` failed on every flesh file ("there is no skeleton … in this program")
    let lib = "skeleton Wallets {\n    memory { bal: Map<String, Int> }\n    face { signal balance(k: String) -> Int  reads bal }\n}\n";
    let flesh = "flesh Wallets {\n    on balance(k) { return bal.get(k) ?? 0 }\n}\n";
    let test = "use app\ncell test T {\n    rules { assert Wallets.balance(\"a\") == 0 }\n}\n";
    let path = project("r7_check_dir", &[("app.cell", "use lib::s\nuse body::f\n"), ("lib/s.cell", lib), ("body/f.cell", flesh), ("test_w.cell", test)]);
    let dir = std::path::Path::new(&path).parent().unwrap().display().to_string();
    let (out, code) = soma(&["check", &dir]);
    assert!(code == 0 && out.contains("0 failed") && out.contains("checked through"), "{out}");
    // a broken body is still found, through the entry
    std::fs::write(std::path::Path::new(&dir).join("body/f.cell"), flesh.replace("bal.get(k) ?? 0", "nope(k)")).unwrap();
    let (out, code) = soma(&["check", &dir]);
    assert!(code != 0 && out.contains("nope"), "{out}");
    // `soma test .` ends with one total
    std::fs::write(std::path::Path::new(&dir).join("body/f.cell"), flesh).unwrap();
    std::fs::write(std::path::Path::new(&dir).join("test_x.cell"), test.replace("cell test T", "cell test U")).unwrap();
    let (out, code) = soma(&["test", &dir]);
    assert!(code == 0 && out.contains("2 assertions: 2 passed, 0 failed"), "{out}");
}

#[test]
fn r7_a_body_left_by_a_rename_is_named_and_helpers_show_what_they_take() {
    let renamed = R6_WALLET.replace("signal put(k: String, n: Int) -> Int  writes bal", "signal store(k: String, n: Int) -> Int  writes bal");
    let (out, code) = soma(&["check", &program("r7_renamed", &renamed)]);
    assert!(code != 0 && out.contains("If the signal was renamed") && out.contains("`store`"), "{out}");
    let (out, _) = soma(&["skeleton", &program("r7_helpers", R6_WALLET), "W"]);
    assert!(out.contains("on _store(k: String, n: Int)"), "{out}");
}

#[test]
fn r7_verify_gives_no_tick_to_the_seventh_rounds_reproducers() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/skeleton_review/round7/red");
    for (dir, tick) in [
        ("F1_match_binder_hides_local", "✓ invariant c >= 0"), ("F1_match_binder_hides_local/requires_variant", "✓ f: requires"),
        ("F2_slot_named_size", "✓ invariant c >= 0"), ("F2_slot_named_size/key_variant", "✓ invariant c >= 0"), ("F2_slot_named_size/slot_len_variant", "✓ invariant c >= 0"),
        ("F3_let_in_if_expression_hides_local", "✓ invariant c >= 0"), ("F3_let_in_if_expression_hides_local/match_arm_variant", "✓ invariant c >= 0"),
        ("F4_guard_proven_across_lowercase_cell_call", "✓ guard"), ("F4_guard_proven_across_lowercase_cell_call/own_cell_variant", "✓ guard"),
        ("F5_guard_proven_across_stored_lambda", "✓ guard"), ("F5_guard_proven_across_stored_lambda/any_variant", "✓ guard"),
        ("F6_requires_proven_emit_site_unseen", "✓ f: requires"), ("F6b_requires_proven_omitted_trailing_map", "✓ f2: requires"),
        ("F7_float_modulo_through_local", "✓ invariant c <= 99"), ("F8_ufcs_count_other_cell_handler", "✓ invariant c <= 9"),
        ("F9_product_lambda_kept_for_later/ps03_lam_index", "✓ before"), ("F9_product_lambda_kept_for_later/ps04_lam_coalesce", "✓ before"),
        ("F9_product_lambda_kept_for_later/ps05_lam_ifexpr", "✓ before"), ("F9_product_lambda_kept_for_later/ps06_lam_map", "✓ before"),
        ("F10_assuming_overflow", "✓ before"), ("F11_pipe_into_slot_write_unseen/size", "✓ invariant log.size"), ("F11_pipe_into_slot_write_unseen/value", "✓ invariant c >= 0"),
    ] {
        let (out, _) = soma(&["verify", &format!("{root}/{dir}/app.cell")]);
        assert!(!out.contains(tick), "{dir}: {out}");
    }
}

#[test]
fn r7_a_method_call_named_like_a_builtin_never_runs_another_cells_handler() {
    // `x.count()` in a pure signal ran `B.count` (which wrote B's memory): no frame said so
    let src = "cell B {\n    memory { seen: Map<String, Int> }\n    on count(v: Float) { seen.set(\"n\", 1)  return v }\n    on got() { return seen.size }\n}\nskeleton A {\n    face { signal w(x: Float) -> Float }\n}\nflesh A {\n    on w(x) { return x.count() }\n}\ncell test T {\n    rules {\n        assert_fails A.w(9.5) matching \"builtin\"\n        assert B.got() == 0\n        assert B.count(9.5) == 9.5\n    }\n}\n";
    let (out, _) = soma(&["test", &program("r7_ufcs_builtin_name", src)]);
    assert!(out.contains("3 passed, 0 failed"), "{out}");
    // a handler of the caller's own cell, or one no builtin is named like, runs as before
    let own = "cell A {\n    on dbl(n: Int) { return n * 2 }\n    on go(n: Int) { return (n + 1).dbl() }\n}\ncell C { on trip(n: Int) { return n * 3 } }\ncell D { on go(n: Int) { return n.trip() } }\ncell test T { rules { assert A.go(1) == 4  assert D.go(2) == 6 } }\n";
    let (out, _) = soma(&["test", &program("r7_ufcs_own", own)]);
    assert!(out.contains("2 passed, 0 failed"), "{out}");
}

#[test]
fn r7_rename_moves_a_signal_everywhere_it_is_written() {
    let lib = "skeleton Wallets {\n    memory { bal: Map<String, Int> }\n    face {\n        signal debit(k: String, n: Int) -> Int  writes bal  calls balance  emits moved\n        signal balance(k: String) -> Int  reads bal\n        signal moved(k: String)\n    }\n}\nskeleton Orders {\n    face {\n        signal pay(id: String, buyer: String) -> Int  calls Wallets.debit, debit\n        signal debit(x: Int) -> Int\n        signal moved(k: String)\n    }\n}\n";
    let flesh = "flesh Wallets {\n    on debit(k, n) {\n        bal.set(k, (bal.get(k) ?? 0) - n)\n        emit moved(k)\n        return balance(k)\n    }\n    on balance(k) { return bal.get(k) ?? 0 }\n    on moved(k) { return () }\n}\nflesh Orders {\n    on pay(id, buyer) {\n        let left = Wallets.debit(buyer, 5)\n        let note = \"{Wallets.debit(buyer, 0)} left\"\n        return left + debit(0)\n    }\n    on debit(x) { return x }\n    on moved(k) { return () }\n}\n";
    let test = "use app\ncell test T {\n    on _setup() { return Wallets.debit(\"a\", 1) }\n    rules {\n        assert Wallets.debit(\"a\", 0) == 0\n        assert Orders.debit(3) == 3\n        assert Orders.pay(\"o\", \"b\") == -5\n    }\n}\n";
    let path = project("r7_rename", &[("app.cell", "use lib::s\nuse body::f\n"), ("lib/s.cell", lib), ("body/f.cell", flesh), ("test_wallets.cell", test)]);
    let dir = std::path::Path::new(&path).parent().unwrap().to_path_buf();
    let (out, code) = soma(&["test", &dir.display().to_string()]);
    assert!(code == 0 && out.contains("3 passed, 0 failed"), "{out}");
    // a dry run changes nothing
    let (out, code) = soma(&["rename", &path, "Wallets.debit", "take", "--dry-run"]);
    assert!(code == 0 && out.contains("lib/s.cell:4") && std::fs::read_to_string(dir.join("lib/s.cell")).unwrap().contains("signal debit(k"), "{out}");
    let (out, code) = soma(&["rename", &path, "Wallets.debit", "take"]);
    assert_eq!(code, 0, "{out}");
    let read = |f: &str| std::fs::read_to_string(dir.join(f)).unwrap();
    // Wallets.debit is `take` everywhere; Orders.debit is another signal and keeps its name
    assert!(read("lib/s.cell").contains("signal take(k: String, n: Int)") && read("lib/s.cell").contains("calls Wallets.take, debit") && read("lib/s.cell").contains("signal debit(x: Int)"), "{}", read("lib/s.cell"));
    assert!(read("body/f.cell").contains("on take(k, n)") && read("body/f.cell").contains("Wallets.take(buyer, 5)") && read("body/f.cell").contains("{Wallets.take(buyer, 0)} left")
        && read("body/f.cell").contains("left + debit(0)") && read("body/f.cell").contains("on debit(x)"), "{}", read("body/f.cell"));
    assert!(read("test_wallets.cell").contains("Wallets.take(\"a\", 1)") && read("test_wallets.cell").contains("Orders.debit(3)"), "{}", read("test_wallets.cell"));
    let (out, code) = soma(&["test", &dir.display().to_string()]);
    assert!(code == 0 && out.contains("3 passed, 0 failed"), "{out}");
    // an event is renamed in every cell that hears it, with its emits
    let (out, code) = soma(&["rename", &path, "Wallets.moved", "shifted"]);
    assert_eq!(code, 0, "{out}");
    assert!(read("lib/s.cell").contains("emits shifted") && !read("lib/s.cell").contains("moved") && !read("body/f.cell").contains("moved"), "{}{}", read("lib/s.cell"), read("body/f.cell"));
    let (out, code) = soma(&["check", &dir.display().to_string()]);
    assert_eq!(code, 0, "{out}");
    // a name already taken is refused
    let (out, code) = soma(&["rename", &path, "Wallets.take", "balance"]);
    assert!(code != 0 && out.contains("already"), "{out}");
}

#[test]
fn r8_verify_gives_no_tick_to_the_eighth_rounds_reproducers() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/skeleton_review/round8");
    for (dir, tick) in [
        ("g_guard_if_condition_effect", "✓ guard"), ("g_guard_assign_in_require_constraint", "✓ guard"), ("g_guard_status_method_form", "✓ guard"),
        ("g_guard_proven_on_hole", "✓ guard"),
        ("prod_require_operand_run_twice", "✓ entering"), ("prod_require_operand_run_twice_requires", "✓ before"),
        ("prod_interpolation_hoisted_before_transition", "✓ entering"), ("prod_interpolation_hoisted_before_transition_requires", "✓ before"),
        ("prod_status_var_read_after_call_same_condition", "✓ entering"), ("prod_status_var_read_after_call_require", "✓ entering"),
        ("prod_vote_runs_handler_unseen", "✓ entering"), ("prod_deliver_resumes_task_unseen", "✓ entering"), ("prod_hole_calls_deliver_tick", "✓ entering"),
        ("prod_interior_cell_same_name_as_top_cell", "✓ entering"),
        ("inv_pattern_string_assign", "✓ invariant c >= 0"), ("inv_pattern_string_write", "✓ invariant c >= 0"), ("inv_require_block_indexset", "✓ invariant c >= 0"),
        ("inv_deep_let_linear", "✓ invariant c >= 0"), ("inv_deep_let_le_fact", "✓ invariant c >= 0"), ("inv_deep_let_expr_fact", "✓ invariant c >= 0"),
        ("inv_deep_let_key_exists", "writer 'w' proven"), ("inv_deep_let_never_commit", "can never commit"),
        ("inv_param_named_like_cell", "writer 'w' proven"), ("inv_slot_named_like_cell", "writer 'w' proven"),
        ("inv_monotone_own_max", "writer 'w' proven"), ("inv_own_keys_handler", "writer 'w' proven"), ("inv_monotone_key_rebound_in_value", "writer 'w' proven"),
        ("inv_key_exists_interpolated_key", "writer 'w' proven"), ("inv_fact_unstable_call_in_interpolation", "writer 'w' proven"),
        ("inv_monotone_write_hidden_in_interpolation", "writer 'w' proven"),
        ("inv_await_carries_size_fact", "writer 'add' proven"), ("inv_await_carries_value_fact", "writer 'add' proven"),
        ("inv_resume_runs_waiting_handler_unseen", "writer 'add' proven"), ("inv_slot_len_delete", "✓ invariant _slot_len"),
        ("con_requires_vote_site_unseen", "✓ f: requires"), ("con_requires_horde_target_site_unseen", "✓ f: requires"), ("con_requires_horde_callback_site_unseen", "✓ f: requires"),
        ("con_requires_local_named_like_cell", "✓ f: requires"), ("con_requires_param_named_like_cell", "✓ f: requires"), ("con_ensures_param_named_like_cell", "✓ h: ensures"),
    ] {
        let (out, _) = soma(&["verify", &format!("{root}/{dir}/app.cell")]);
        assert!(!out.contains(tick), "{dir}: {out}");
    }
    // a rule that is only a `while` is a rule: its ✓ is not followed by "lists no state"
    let (out, code) = soma(&["verify", &format!("{root}/prod_while_only_rule_verify_failed/app.cell")]);
    assert!(code == 0 && out.contains("✓ entering") && out.contains("VERIFY OK"), "{out}");
    // `std.count(5, 1, 2, 3, 4)`: the builtin's refusal, not another cell's handler `count`
    let (out, _) = soma(&["test", &format!("{root}/tool_std_prefix_runs_other_cell_handler/app.cell")]);
    assert!(out.contains("assert B.seen() == 0") && !out.contains("✗ ") , "{out}");
}

#[test]
fn r8_the_directory_gate_and_refs_miss_nothing_the_eighth_round_found() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/skeleton_review/round8");
    // `soma check <dir>`: red where a file is red (a part with tests of its own, two
    // files importing each other, a file importing itself, an entry reached only from tests)…
    for dir in ["tool_check_dir_part_inline_tests", "tool_check_dir_cycle_skipped", "tool_check_dir_self_import_skipped", "tool_check_dir_entry_through_tests_loses_manifest"] {
        let (out, code) = soma(&["check", &format!("{root}/{dir}")]);
        assert!(code != 0, "{dir}: {out}");
    }
    // …and green where the program is green (`use fl` beside the entry, `use lib::b` from lib/)
    for dir in ["tool_check_dir_use_sibling_red", "tool_check_dir_lib_imports_lib_red"] {
        let (out, code) = soma(&["check", &format!("{root}/{dir}")]);
        assert_eq!(code, 0, "{dir}: {out}");
    }
    // refs: method form, a bare call from another cell, vote / horde / delegate, nested strings
    let (out, _) = soma(&["refs", &format!("{root}/tool_refs_missed_calls/app.cell"), "B.tick"]);
    assert!(out.lines().filter(|l| l.contains("app.cell:")).count() >= 11, "{out}");
    let (out, _) = soma(&["refs", &format!("{root}/tool_refs_missed_calls_tests/app.cell"), "B.tick"]);
    for needle in ["tests/deep/er/t.cell:3", "tests/deep/er/t.cell:5", "tests/deep/er/t.cell:14", "target/t2.cell:3"] { assert!(out.contains(needle), "{needle}: {out}"); }
    // a file the program never imports is not the program; a test that does not parse is named
    let (out, _) = soma(&["refs", &format!("{root}/tool_refs_foreign_file_and_broken_test/app.cell"), "Stock.level"]);
    assert!(!out.contains("app_backup") && out.contains("does not parse"), "{out}");
}

#[test]
fn r8_the_analyses_around_the_skeleton_give_no_tick_either() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/skeleton_review/round8");
    // two machines kept under one key: refused at check
    for dir in ["g_sm_storage_key_collision", "g_sm_storage_key_collision/legacy_key_variant"] {
        let (out, code) = soma(&["check", &format!("{root}/{dir}/app.cell")]);
        assert!(code != 0 && out.contains("share") || out.contains("keeps its own"), "{dir}: {out}");
    }
    for (dir, tick) in [
        ("g_waits_helper_unseen", "✓ refund() wait"), ("g_effects_tools_allowed_second_think", "can dispatch [lookup]\n"),
        ("g_composition_emit_in_try", "0 undelivered"),
    ] {
        let (out, _) = soma(&["verify", &format!("{root}/{dir}/app.cell")]);
        assert!(!out.contains(tick), "{dir}: {out}");
    }
    for (dir, tick) in [
        ("con_cost_latency_std_sleep_unseen", "latency bound proven"), ("con_cost_latency_ufcs_sleep_unseen", "latency bound proven"),
        ("con_cost_local_named_like_cell", "bound proven"), ("g_deterministic_shadowed_local", "All checks passed"),
    ] {
        let (out, _) = soma(&["check", &format!("{root}/{dir}/app.cell")]);
        assert!(!out.contains(tick), "{dir}: {out}");
    }
    // an attestation names every file of the program: an imported one too
    let work = std::env::temp_dir().join("soma_skeleton_r8_attest");
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).unwrap();
    for f in ["app.cell", "side.cell", "soma.toml"] {
        let from = std::path::Path::new(root).join("g_attest_unhashed_import").join(f);
        if from.exists() { std::fs::copy(from, work.join(f)).unwrap(); }
    }
    let att = work.join("out.json").display().to_string();
    let (out, _) = soma(&["verify", "--attest", &att, &work.join("app.cell").display().to_string()]);
    let text = std::fs::read_to_string(&att).unwrap_or_default();
    assert!(text.contains("side.cell"), "{out}\n{text}");
}

#[test]
fn r9_a_rename_that_succeeds_leaves_check_and_test_as_they_were() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/skeleton_review/round9");
    let mut tried = 0;
    for entry in std::fs::read_dir(&root).unwrap().flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with("c_rename") { continue; }
        // the command its README gives: `soma rename app.cell Cell.signal new`
        let readme = std::fs::read_to_string(entry.path().join("README")).unwrap_or_default();
        let words: Vec<&str> = readme.split_whitespace().collect();
        let Some(at) = words.iter().position(|w| *w == "rename") else { continue };
        let (target, new) = (words[at + 2], words[at + 3]);
        let work = std::env::temp_dir().join(format!("soma_skeleton_r9_{name}"));
        let _ = std::fs::remove_dir_all(&work);
        assert!(Command::new("cp").arg("-R").arg(entry.path()).arg(&work).status().unwrap().success());
        let dir = work.display().to_string();
        let verdicts = |dir: &str| (soma(&["check", dir]).1 == 0, soma(&["test", dir]).1 == 0);
        let before = verdicts(&dir);
        let (out, code) = soma(&["rename", &work.join("app.cell").display().to_string(), target, new]);
        if code == 0 { assert_eq!(verdicts(&dir), before, "{name}: {out}\n{}", soma(&["check", &dir]).0); }
        tried += 1;
    }
    assert!(tried >= 14, "{tried}");
}

#[test]
fn r9_no_tick_and_no_leak_for_the_ninth_rounds_reproducers() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/skeleton_review/round9");
    for (dir, tick) in [
        ("a1_method_args_before_target", "✓ entering"), ("a1_index_before_target", "✓ entering"), ("a1_match_pattern_interpolation_hoisted", "✓ entering"),
        ("a1_narrow_condition_permutes_states", "✓ entering"), ("a1_kept_lambda_calls_builtin_named_handler", "✓ entering"),
        ("a1_kept_lambda_call_in_interpolation", "✓ entering"), ("a1_local_named_like_cell_ufcs", "✓ entering"),
        ("a1_delegate_literal_with_local_named_like_cell", "✓ entering"), ("a1_vote_string_target", "✓ entering"),
        ("a1_task_transition_target_in_variable", "✓ entering"), ("a1_task_transition_in_interpolation", "✓ entering"),
        ("a2_monotone_default_mismatch", "writer 'w' proven"), ("a2_monotone_strict_float_absorption", "writer 'w' proven"),
        ("a2_require_text_matches_invariant_binding", "writer 'w' proven"), ("a2_fact_text_elides_bodies", "writer 'w' proven"),
        ("a2_guard_text_elides_bodies", "✓ guard"), ("a2_effects_think_in_indexset", "can dispatch [lookup]\n"),
        ("a2_waits_report_condition_and_argument_unseen", "nothing is written before it"), ("a2_waits_report_wrong_resting_state", "✓ refund() wait"),
    ] {
        let (out, _) = soma(&["verify", &format!("{root}/{dir}/app.cell")]);
        assert!(!out.contains(tick), "{dir}: {out}");
    }
    // what ran another cell's handler — or the cell's own — under a frame that did not say so:
    // each reproducer's test asserted the leak, and no longer passes as written
    for dir in ["a1_handler_in_cell_type_unseen", "a1_std_prefix_runs_own_handler_unseen_by_product", "a1_hole_11_std_own_pure/filled", "a1_hole_12_type_cell/filled"] {
        let (out, _) = soma(&["test", &format!("{root}/{dir}/app.cell")]);
        assert!(out.contains("0 passed"), "{dir}: {out}");
    }
    // declared wrongly, refused at check
    for dir in ["b_slot_named_like_cell_write_read_as_call", "b_slot_named_like_machine_read_under_moves", "b_vote_string_target_undeclared_call"] {
        let (out, code) = soma(&["check", &format!("{root}/{dir}/app.cell")]);
        assert!(code != 0 && out.contains("E0120"), "{dir}: {out}");
    }
    // the directory gate and the index
    let (out, code) = soma(&["check", &format!("{root}/c_dir_import_check_red_refs_blind")]);
    assert_eq!(code, 0, "{out}");
    let (out, code) = soma(&["check", &format!("{root}/c_check_dir_pkg_shadow_false_green")]);
    assert!(code != 0, "{out}");
    let (out, _) = soma(&["skeleton", &format!("{root}/c_index_plain_cells_zero_signals/app.cell"), "--index"]);
    assert!(!out.contains("0 signals") && out.contains("7 handlers"), "{out}");
}

#[test]
fn r10_where_two_components_disagreed_no_reproducer_keeps_both_its_tick_and_its_test() {
    // each reproducer of round 10 pairs a ✓ with a passing test that shows the
    // contrary: closed when the ✓ is gone, or when the run no longer does what
    // the analysis could not see (the test then fails as written)
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/skeleton_review/round10");
    let ticks = ["✓ entering", "✓ before", "✓ guard", "writer 'w' proven", "writer 'dec' proven", "writer 'w2' proven", "✓ f: requires", "bound proven", "✓ refinement: handler `go` ⟶ {a → c"];
    // (a `cell test` handler that listens to an event moves the machines of its own test: by design)
    let by_design = ["q6_emit_heard_by_test_cell_handler_also_under_run"];
    let mut seen = 0;
    for entry in std::fs::read_dir(&root).unwrap().flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let app = entry.path().join("app.cell");
        if !app.exists() || name.contains("tool_") || name.contains("refs_") || name.contains("leak_") || by_design.contains(&name.as_str()) { continue; }
        let app = app.display().to_string();
        let (test, _) = soma(&["test", &app]);
        let green = test.lines().last().is_some_and(|l| l.contains(", 0 failed"));
        let said = format!("{}{}", soma(&["verify", &app]).0, soma(&["check", &app]).0);
        let ticked: Vec<&&str> = ticks.iter().filter(|t| said.contains(**t)).collect();
        assert!(!green || ticked.is_empty(), "{name}: {ticked:?}\n{test}");
        seen += 1;
    }
    assert!(seen >= 40, "{seen}");
    // the leaks: refused at check, or the effect is gone from the run
    for dir in ["q3_assign_binds_local_named_like_cell", "q3_or_pattern_alternative_binds_local_named_like_cell"] {
        let (out, code) = soma(&["check", &root.join(dir).join("app.cell").display().to_string()]);
        assert!(code != 0 && out.contains("E0120"), "{dir}: {out}");
    }
    for dir in ["q1_leak_interpolated_pipe_into_local_field", "q1_leak_delegate_computed_name_runs_private_helper"] {
        let (out, _) = soma(&["test", &root.join(dir).join("app.cell").display().to_string()]);
        assert!(!out.lines().last().is_some_and(|l| l.contains(", 0 failed")), "{dir}: {out}");
    }
    let (out, _) = soma(&["refs", &root.join("q1_tool_refs_rename_miss_string_targets/app.cell").display().to_string(), "B.bump"]);
    assert!(out.contains("A.a08") && out.contains("A.a09") && out.contains("A.a18"), "{out}");
}

#[test]
fn r11_the_eleventh_rounds_disagreements_are_settled() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/skeleton_review/round11");
    for (dir, tick) in [
        ("z1_effects_tools_allowed_read_at_wrong_position", "handler `ask` calls think() at 1 site (the cost line counts loop rounds) — can dispatch [lookup]\n"),
        ("z1_termination_tool_recursion_hidden_by_tools_allowed_position", "✓ termination"),
        ("z1_effects_tools_allowed_not_transitive", "handler `ask` calls think() at 1 site (the cost line counts loop rounds) — can dispatch [lookup]\n"),
        ("z2_runtime_section_statements_unseen_by_verify", "✓ before"), ("z2_runtime_section_statements_unseen_by_verify", "✓ guard"),
        ("z2_runtime_section_statements_unseen_by_verify", "✓ f: requires"),
        ("z3_product_task_callee_stops_at_await", "✓ before"),
        // a ✓ on a skeleton holds for every flesh that conforms: a body may repeat and reorder what its frame allows
        ("z5_hole_model_calls_once/hole", "✓ entering"), ("z5_hole_model_edge_order/hole", "✓ entering"),
    ] {
        let (out, _) = soma(&["verify", &format!("{root}/{dir}/app.cell")]);
        assert!(!out.contains(tick), "{dir}: {out}");
    }
    // a handler run by `vote` / `horde` under a bare name, a horde's callbacks: this signal's effects
    for dir in ["zA_leak_vote_horde_bare_handler_name", "z3_leak_horde_callbacks_private_helpers"] {
        let (out, code) = soma(&["check", &format!("{root}/{dir}/app.cell")]);
        assert!(code != 0 && out.contains("E0120"), "{dir}: {out}");
    }
    // what a model answered decides no state, in method form either
    for dir in ["z4_taint_source_forms_unseen", "z4_taint_sink_forms_unseen"] {
        let (out, code) = soma(&["check", &format!("{root}/{dir}/app.cell")]);
        assert!(code != 0 && out.contains("think()"), "{dir}: {out}");
    }
}

#[test]
fn r12_one_answer_where_there_were_several() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/skeleton_review/round12");
    // refused at check: a handler reached by a name the frame could not see
    for dir in ["a2_leak_vote_horde_target_local_named_like_handler", "a2_leak_vote_horde_bare_name_other_cell_private_helper",
        "a3_check_cost_taint_blind_to_runtime_section", "a3_skeleton_runtime_section_outside_frames",
        "b3_timer_calls_private_helper_of_other_skeleton", "b2_capabilities_requires_forms_unseen", "b5_cost_parent_emit_reaches_interior_children"] {
        let (out, code) = soma(&["check", &format!("{root}/{dir}/app.cell")]);
        assert!(code != 0, "{dir}: {out}");
    }
    for (dir, tick) in [
        ("a4_effects_tools_allowed_one_level_only", "handler `ask` calls think() at 1 site (the cost line counts loop rounds) — can dispatch [lookup]\n"),
        ("b1_guard_interpolated_literal_read_as_constant", "✓ guard"), ("b1_guard_fact_on_bare_slot_survives_write", "✓ guard"),
        ("b1_bigint_type_name_unchecked_at_run", "writer 'put' proven"), ("b6_invariant_fact_through_lowercase_cell_call", "✓ invariant c <= 10"),
        ("b5_product_parent_emit_reaches_interior_children", "✓ entering"), ("b5_product_bare_call_resolved_to_interior_homonym", "✓ entering"),
        ("b5_requires_site_bare_call_resolved_to_interior_homonym", "proven at"),
    ] {
        let (out, _) = soma(&["verify", &format!("{root}/{dir}/app.cell")]);
        assert!(!out.contains(tick), "{dir}: {out}");
    }
    // `-> Int` gives an Int: the arithmetic a proof read is the arithmetic that runs
    let src = "cell A {\n    face { signal scale(n: Int) -> Int }\n    on scale(n: Int) { return pow(2, n) }\n    on step(n: Int) { let a = scale(n)  let b = a + 1  return b - a }\n}\ncell test T { rules { assert A.step(60) == 1  assert A.scale(10) == 1024 } }\n";
    let (out, _) = soma(&["test", &program("r12_int_return", src)]);
    assert!(out.contains("2 passed, 0 failed"), "{out}");
    // tools: an entry in a sub-folder, the `runtime { }` section, an imported folder read one level deep
    let (out, _) = soma(&["refs", &format!("{root}/b7_refs_rename_entry_in_subfolder/before/tests/unit/orders_test.cell"), "Stock.take"]);
    assert!(out.contains("Orders.place"), "{out}");
    let (out, _) = soma(&["refs", &format!("{root}/b7_refs_runtime_section/app.cell"), "Stock.fill"]);
    assert!(out.contains("runtime { }"), "{out}");
    let (out, code) = soma(&["check", &format!("{root}/b7_dir_import_depth")]);
    assert!(code != 0, "{out}");
}

#[test]
fn a_comment_changed_after_the_freeze_is_said_not_refused() {
    // the comment of a signal is its brief: changing it changes what a body must do
    let src = "skeleton L {\n    memory { cash: Map<String, Int> }\n    face {\n        // Pays `amount` in cash. Returns the amount left due.\n        signal pay(folio: String, amount: Int) -> Int  writes cash\n        signal due(folio: String) -> Int  reads cash\n    }\n}\n";
    let path = program("lock_comment", src);
    let (out, code) = soma(&["skeleton", &path, "--freeze"]);
    assert_eq!(code, 0, "{out}");
    let (out, code) = soma(&["check", &path]);
    assert!(code == 0 && !out.contains("changed since the freeze"), "{out}");
    std::fs::write(&path, src.replace("Returns the amount left due.", "More than is due: error kind `Overpaid`.")).unwrap();
    let (out, code) = soma(&["check", &path]);
    assert!(code == 0 && out.contains("warning") && out.contains("the comment of `L.pay` changed since the freeze"), "{out}");
    assert!(!out.contains("comment of `L.due`"), "{out}");
    // amending records it
    let (out, code) = soma(&["skeleton", &path, "--amend"]);
    assert_eq!(code, 0, "{out}");
    let (out, code) = soma(&["check", &path]);
    assert!(code == 0 && !out.contains("changed since the freeze"), "{out}");
}

#[test]
fn s4_one_rule_for_who_hears_an_event_and_one_store_per_slot() {
    // a top-level cell's event: the top-level cells and its own interior;
    // an interior cell's: its siblings and its parent — whatever started the program
    let src = "cell Log {\n    memory { got: Map<String, Int> [persistent] }\n    on ping(k: String) { got.set(\"log\", (got.get(\"log\") ?? 0) + 1)  return 1 }\n    on n(k: String) { return got.get(k) ?? 0 }\n}\ncell Desk {\n    memory { seen: Map<String, Int> [persistent] }\n    interior {\n        cell Front {\n            on ping(k: String) { emit pong(k)  return 1 }\n        }\n    }\n    on go(k: String) { emit ping(k)  return 1 }\n    on pong(k: String) { seen.set(k, (seen.get(k) ?? 0) + 1)  return 1 }\n    on count(k: String) { return seen.get(k) ?? 0 }\n    runtime { go(\"k\") }\n}\n";
    let path = program("s4_events", src);
    let dir = std::path::Path::new(&path).parent().unwrap().to_path_buf();
    let run = |args: &[&str]| { let mut a = vec!["run", path.as_str()]; a.extend_from_slice(args); let o = Command::new(env!("CARGO_BIN_EXE_soma")).args(&a).current_dir(&dir).output().unwrap(); String::from_utf8_lossy(&o.stdout).trim().to_string() };
    let _ = Command::new(env!("CARGO_BIN_EXE_soma")).args(["run", "--fresh", &path, "count", "k"]).current_dir(&dir).output();
    // the runtime section: `go` emits `ping` — heard by Log (top level) and by Front (Desk's interior),
    // whose `pong` is heard by its parent Desk
    run(&[]);
    assert_eq!(run(&["count", "k"]), "1", "the interior's event reached its parent, in the store `count` reads");
    assert_eq!(run(&["Log.n", "log"]), "1", "a top-level event reached the top-level cells under `soma run app.cell` too");
}
