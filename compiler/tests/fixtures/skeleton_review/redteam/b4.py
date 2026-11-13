from gen import *
import gen
OTHER='cell Other {\n    memory { o: Map<String, Int> }\n    on poke(id: String) { o.set(id, 7) return 7 }\n    on seen(id: String) { return o.get(id) ?? 0 }\n}\n'
case("c40_foreign_slot_stmt", '    on pure(id) { Other.o.set(id, 5) return 1 }\n', extra=OTHER, tests=['assert pure("k") == 1','assert Other.seen("k") == 0'])
case("c41_foreign_slot_expr", '    on pure(id) { let r = Other.o.set(id, 5) return 1 }\n', extra=OTHER, tests=['assert pure("k") == 1','assert Other.seen("k") == 0'])
case("c42_foreign_slot_index", '    on pure(id) { Other.o[id] = 5 return 1 }\n', extra=OTHER, tests=['assert pure("k") == 1','assert Other.seen("k") == 0'])
case("c43_foreign_slot_read", '    on pure(id) { return Other.o.get(id) ?? 3 }\n', extra=OTHER, tests=['assert Other.poke("k") == 7', 'assert pure("k") == 3'])
case("c34b_interior_same_name", '    on pure(id) { acct.set(id, 99) return 1 }\n', pre='cell Outer {\n    interior {\n        cell Bank {\n            face { signal pure(id: String) -> Int }\n            on pure(id: String) { return 1 }\n        }\n        cell Src { face { signal go() } on go() { emit pure("a") } }\n    }\n    on run() { return 1 }\n}\n')
gen.SK=gen.SK.replace("new","New").replace("paid","Paid").replace("shipped","Shipped")
gen.BASE='''    on peek(id) { return acct.get(id) ?? 0 }
    on bump(id) { acct.set(id, (acct.get(id) ?? 0) + 1) return 1 }
    on status(id) { return get_status(id) ?? "New" }
'''
T=['assert pay("k") == "x"','assert status("k") == "Paid"']
P='    on pure(id) { return 1 }\n'
case("c13c_for_var_state", '    on pay(id) { transition(id, "New", "Paid") for Paid in list("Shipped") { transition(id, Paid) } return "x" }\n'+P, tests=T)
case("c13d_lambda_param_state", '    on pay(id) { transition(id, "New", "Paid") let r = map(list("Shipped"), Paid => transition(id, Paid)) return "x" }\n'+P, tests=T)
case("c13e_match_binder_state", '    on pay(id) { transition(id, "New", "Paid") let r = match "Shipped" { Paid -> transition(id, Paid) } return "x" }\n'+P, tests=T)
