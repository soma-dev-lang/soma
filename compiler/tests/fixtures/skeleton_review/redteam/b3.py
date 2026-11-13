from gen import *
import gen
W='    on _w(id: String) { acct.set(id, 99) return 1 }\n'
case("c02_emit_reaches_private_helper", '    on pure(id) { return 1 }\n    on aux1(id) { return _w(id) }\n'+W, face='        signal aux1(id: String) -> Int  writes acct\n', extra='cell Other {\n    on poke(id: String) { emit _w(id) return 1 }\n}\n', tests=['assert Other.poke("k") == 1','assert peek("k") == 0'])
case("c34_interior_same_name", '    on pure(id) { acct.set(id, 99) return 1 }\n', pre='cell Outer {\n    interior {\n        cell Bank {\n            on pure(id: String) { return 1 }\n        }\n    }\n    on run() { return 1 }\n}\n')
# capitalised states
gen.SK='''skeleton Bank {
    memory { acct: Map<String, Int> [persistent] }
    state order {
        initial: New
        New -> Paid
        Paid -> Shipped
    }
    face {
        signal peek(id: String) -> Int      reads acct
        signal bump(id: String) -> Int      writes acct
        signal pay(id: String) -> String    moves New -> Paid
        signal status(id: String) -> String reads order
        signal pure(id: String) -> Int
%FACE%
    }
}
'''
gen.BASE='''    on peek(id) { return acct.get(id) ?? 0 }
    on bump(id) { acct.set(id, (acct.get(id) ?? 0) + 1) return 1 }
    on status(id) { return get_status(id) ?? "New" }
'''
case("c13_state_ident_local", '    on pay(id) { transition(id, "New", "Paid") let Paid = "Shipped" transition(id, Paid) return "x" }\n    on pure(id) { return 1 }\n', tests=['assert pay("k") == "x"','assert status("k") == "Paid"'])
case("c13b_state_ident_sanity", '    on pay(id) { transition(id, "New", "Paid") transition(id, "Shipped") return "x" }\n    on pure(id) { return 1 }\n', tests=['assert pay("k") == "x"'])
case("c14_state_ident_helper_param", '    on pay(id) { transition(id, "New", "Paid") return _go(id, "Shipped") }\n    on _go(id: String, Paid: String) { transition(id, Paid) return "x" }\n    on pure(id) { return 1 }\n', tests=['assert pay("k") == "x"','assert status("k") == "Paid"'])
