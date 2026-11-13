exec(open('mk.py').read().split('E3=')[0])
w("V2_std_transition", APPR+pay("        due -> paid","moves * -> *",
'''        std.transition(id, "due", "paid")
        return 1''')+TEST)
w("V5_guard_proven_effect_in_same_statement", '''skeleton G {
    memory { cap: Map<String, Int> [persistent] }
    state m {
        initial: a
        a -> b { guard { (cap.get("x") ?? 0) > 0 } }
    }
    face {
        signal open() -> Int             writes cap
        signal go(id: String) -> Int     writes cap   moves a -> b
    }
}
flesh G {
    on open() { cap.set("x", 5)  return 1 }
    on _zid(s: String) { cap.set("x", 0)  return s }
    on go(id) {
        require (cap.get("x") ?? 0) > 0 else Closed "closed"
        transition(_zid(id), "a", "b")          // the argument zeroes the slot the guard reads
        return 1
    }
}
cell test T {
    rules {
        assert G.open() == 1
        assert (try { G.go("k") }).kind == "guard_failed"
    }
}
''', toml='[package]\nname = "g"\nedition = "2027"\n', pat=r'guard `')
w("V5b_guard_proven_effect_in_if_condition", '''skeleton G {
    memory { cap: Map<String, Int> [persistent] }
    state m {
        initial: a
        a -> b { guard { (cap.get("x") ?? 0) > 0 } }
    }
    face {
        signal open() -> Int             writes cap
        signal go(id: String) -> Int     writes cap   moves a -> b
    }
}
flesh G {
    on open() { cap.set("x", 5)  return 1 }
    on _zero() { cap.set("x", 0)  return 1 }
    on go(id) {
        require (cap.get("x") ?? 0) > 0 else Closed "closed"
        if _zero() == 1 { transition(id, "a", "b") }
        return 1
    }
}
cell test T {
    rules {
        assert G.open() == 1
        assert (try { G.go("k") }).kind == "guard_failed"
    }
}
''', toml='[package]\nname = "g"\nedition = "2027"\n', pat=r'guard `')
w("V2b_std_transition_guard_proven", '''skeleton G {
    state m {
        initial: a
        a -> b { guard { amount > 0 } }
    }
    face {
        signal ok(id: String, amount: Int) -> Int    moves a -> b
        signal bad(id: String, amount: Int) -> Int   moves * -> *
    }
}
flesh G {
    on ok(id, amount) { require amount > 0 else Bad "x"  transition(id, "a", "b")  return 1 }
    on bad(id, amount) { std.transition(id, "a", "b")  return 1 }
}
cell test T {
    rules { assert (try { G.bad("k", 0 - 5) }).kind == "guard_failed" }
}
''', toml='[package]\nname = "g"\nedition = "2027"\n', pat=r'guard `|refinement')
w("V2c_std_delegate_termination", '''cell G {
    on go(n: Int) { return std.delegate("G", "go", n) }
}
cell test T {
    rules { assert (try { G.go(1) }).error != () }
}
''', toml='[package]\nname = "g"\nedition = "2027"\n', pat=r'terminat')
w("V6_assuming_float", APPR+'''skeleton Pay {
    state pay {
        initial: due
        due -> paid { guard { amount < 6 } }
        due -> ready
        ready -> paid
    }
    face {
        signal fast(id: String, amount: Float) -> Int   moves due -> paid
        signal slow(id: String) -> Int                  moves due -> ready   calls Approval.sign
        signal fin(id: String, amount: Float) -> Int    moves ready -> paid
        signal pst(id: String) -> String                reads pay
    }
}
flesh Pay {
    on fast(id, amount) { transition(id, "due", "paid")  return 1 }
    on slow(id) { Approval.sign(id)  transition(id, "due", "ready")  return 1 }
    on fin(id, amount) { transition(id, "ready", "paid")  return 1 }
    on pst(id) { return get_status(id) ?? "due" }
}
cell test T {
    rules {
        assert Pay.fast("k", 5.5) == 1                  // 5.5 > 5: the assumption holds
        assert Pay.pst("k") == "paid"
        assert Approval.status("k") == "pending"
    }
}
''', toml=TOML+'assuming = "amount > 5"\n')
PL='''cell Approval {
    state appr {
        initial: pending
        pending -> signed_off
    }
    on sign(id: String) { transition(id, "pending", "signed_off")  return 1 }
    on status(id: String) { return get_status(id) ?? "pending" }
}
cell Pay {
    state pay {
        initial: due
        due -> ready
        ready -> paid
        ready -> due
    }
    on pay(id: String) {
        require (get_status(id) ?? "due") == "due" else Busy "busy"
        let f = x => transition(id, "ready", "paid")      // written while `due`…
        transition(id, "due", "ready")
        f(1)                                              // …called while `ready`
        if get_status(id) == "ready" { transition(id, "ready", "due") }
        return 1
    }
    on pst(id: String) { return get_status(id) ?? "due" }
}
'''
w("V7_lambda_called_later", PL+TEST)
