import os, subprocess
SOMA="/Users/antoine/soma/compiler/target/release/soma"; ORACLE="/Users/antoine/soma/tools/frame_oracle.py"
ROOT=os.path.dirname(os.path.abspath(__file__))
def put(name, src, toml=None):
    d=os.path.join(ROOT,name); os.makedirs(d,exist_ok=True)
    open(os.path.join(d,"app.cell"),"w").write(src)
    if toml: open(os.path.join(d,"soma.toml"),"w").write(toml)
    env=dict(os.environ,NO_COLOR="1")
    c=subprocess.run([SOMA,"check","app.cell"],cwd=d,env=env,capture_output=True,text=True)
    t=subprocess.run([SOMA,"test","app.cell"],cwd=d,env=env,capture_output=True,text=True)
    o=subprocess.run(["python3",ORACLE,"--soma",SOMA,os.path.join(d,"app.cell")],capture_output=True,text=True)
    print("==",name,"| check rc",c.returncode,"|",(t.stdout+t.stderr).strip().splitlines()[-1],"|"," ; ".join(l.strip() for l in o.stdout.splitlines()[1:]))
    if c.returncode: print((c.stdout+c.stderr)[:600])

LAM='''skeleton A {
    memory { s: Map<String, Int> [persistent] }
    face {
        signal go(id: String) -> Int     writes s  calls B.apply
    }
}
skeleton B {
    memory { s: Map<String, Int> [persistent] }
    face {
        signal apply(f: Any) -> Int      // no clause: pure
        signal peek(id: String) -> Int   reads s
    }
}
flesh A {
    on go(id) { %BODY% }
}
flesh B {
    on apply(f) { let r = f("j")  return 1 }
    on peek(id) { return s.get(id) ?? 0 }
}
cell test T {
    rules {
        assert A.go("k") == 1
        assert B.peek("j") == 7      // B's slot was written under `B.apply`, a pure signal
    }
}
'''
for n,b in [("A1a_lambda_in_list", 'let fs = [x => s.set(x, 7)]  return B.apply(fs[0])'),
            ("A1b_lambda_alias", 'let f = x => s.set(x, 7)  let g = f  return B.apply(g)'),
            ("A1c_lambda_assigned", 'let f = 0  f = x => s.set(x, 7)  return B.apply(f)'),
            ("A1d_lambda_if_expr", 'let f = if id == "k" { x => s.set(x, 7) } else { x => 1 }  return B.apply(f)'),
            ("A1e_block_lambda_index_set", 'return B.apply(x => { s[x] = 7  1 })'),
            ("A1f_lambda_curried", 'let f = a => b => s.set(b, 7)  let g = f(1)  return B.apply(g)'),
            ("A1g_lambda_shadowed_name", 'let f = x => s.set(x, 7)  if id == "zz" { let f = x => 1 }  return B.apply(f)'),
            ]:
    put(n, LAM.replace("%BODY%", b))
put("A1h_lambda_returned_via_local", '''skeleton B {
    memory { secret: Map<String, Int> [persistent] }
    face {
        signal mk(id: String) -> Any       // no clause: pure
        signal runit(id: String) -> Int    calls mk
        signal peek(id: String) -> Int     reads secret
    }
}
flesh B {
    on mk(id) { let fs = [x => secret.set(x, 7)]  return fs }     // round 2 L4, one indirection further
    on runit(id) { let fs = mk(id)  let f = fs[0]  let r = f("k")  return 1 }
    on peek(id) { return secret.get(id) ?? 0 }
}
cell test T {
    rules {
        assert B.runit("k") == 1
        assert B.peek("k") == 7
    }
}
''')
Q='''skeleton Q {
    state fl {
        initial: a
        a -> b
        a -> x
        b -> x
    }
    face {
        signal st(id: String) -> String   reads fl
        signal ab(id: String) -> Int      moves a -> b
        signal mv(id: String) -> Int      moves %MOVES%  calls ab%CALLS%
    }
}
%PRE%flesh Q {
    on st(id) { return get_status(id) ?? "a" }
    on ab(id) { transition(id, "a", "b")  return 1 }
    on mv(id) {
        %BODY%
    }
}
cell test T {
    rules {
        %SETUP%assert Q.mv("k") == 1
        assert Q.st("%WHO%") == "x"
    }
}
'''
def q(n, body, moves, who="k", setup="", calls="", pre="", toml=None):
    put(n, Q.replace("%BODY%",body).replace("%MOVES%",moves).replace("%WHO%",who).replace("%SETUP%",setup).replace("%CALLS%",calls).replace("%PRE%",pre), toml)
# frame grants only b -> x ; the run takes a -> x on another instance
q("A3a_id_rebound_by_let", 'if (get_status(id) ?? "a") != "b" { return 0 }\n        let id = "other"                 // the fact was about the old `id`\n        transition(id, "x")              // check: b -> x.  run: a -> x\n        return 1', "b -> x", who="other", setup='assert Q.ab("k") == 1\n        ')
q("A3b_index_changes", 'let ids = [id, "other"]  let i = 0\n        if (get_status(ids[i]) ?? "a") != "b" { return 0 }\n        i = i + 1\n        transition(ids[i], "x")          // check: b -> x.  run: a -> x\n        return 1', "b -> x", who="other", setup='assert Q.ab("k") == 1\n        ')
q("A3c_match_binder_rebinds_id", 'if (get_status(id) ?? "a") != "b" { return 0 }\n        let r = match "other" { id -> transition(id, "x") }\n        return 1', "b -> x", who="other", setup='assert Q.ab("k") == 1\n        ')
# frame grants only a -> x ; the run takes b -> x
q("A3d_stale_alias", 'let s = get_status(id) ?? "a"\n        transition(id, "a", "b")\n        if s == "a" { transition(id, "x") }     // s is stale: check a -> x, run b -> x\n        return 1', "a -> b, a -> x")
q("A3e_stale_alias_after_call", 'let s = get_status(id) ?? "a"\n        let r = ab(id)\n        if s == "a" { transition(id, "x") }     // check a -> x, run b -> x\n        return 1', "a -> x")
q("A3f_call_in_same_condition", 'if (get_status(id) ?? "a") == "a" && ab(id) == 1 { transition(id, "x") }   // check a -> x, run b -> x\n        return 1', "a -> x")
q("A3g_lambda_run_by_builtin", 'if (get_status(id) ?? "a") != "a" { return 0 }\n        let r = [1].map(n => transition(id, "a", "b"))\n        transition(id, "x")              // check a -> x, run b -> x\n        return 1', "a -> b, a -> x")
q("A3h_cell_signal_named_like_slot_method", 'if (get_status(id) ?? "a") != "a" { return 0 }\n        Mover.push(id)                   // a signal of another cell, named like a slot method\n        transition(id, "x")              // check a -> x, run b -> x\n        return 1', "a -> x", calls=", Mover.push", pre='skeleton Mover {\n    face { signal push(id: String) -> Int   moves Q.fl: a -> b }\n}\nflesh Mover {\n    on push(id) { transition(id, "a", "b")  return 1 }\n}\n')
q("A3i_alias_from_untaken_branch", 'let s = "b"\n        if id == "never" { let s = get_status(id) }     // not taken: s is still the literal "b"\n        if s == "b" { transition(id, "x") }             // check b -> x, run a -> x\n        return 1', "b -> x")
q("A3j_match_on_defaulted_status_2027", 'let r = match get_status(id) ?? "b" {\n            "b" -> transition(id, "x")       // a fresh id answers the default "b": check b -> x, run a -> x\n            _ -> 0\n        }\n        return 1', "b -> x", toml='[package]\nname = "x"\nedition = "2027"\n')
