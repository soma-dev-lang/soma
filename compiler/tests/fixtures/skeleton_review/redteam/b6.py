from gen import *
P='    on pure(id) { return 1 }\n'
SHOP='skeleton Shop {\n    face {\n        signal settle(id: String) -> Int   moves Bank.order: new -> paid\n        signal look(id: String) -> String  reads Bank.order\n    }\n}\nflesh Shop {\n    on settle(id) { transition(id, "new", "paid") return 1 }\n    on look(id) { return get_status(id) ?? "new" }\n}\n'
case("fp16_cross_cell_moves", P, extra=SHOP, tests=['assert Shop.settle("k") == 1','assert status("k") == "paid"','assert Shop.look("k") == "paid"'])
case("fp04_json_text", '    on pure(id) { let s = "{\\"acct\\": 1}" let t = "css .a {acct: 1}" return len(s) }\n', tests=['assert pure("k") > 0'])
case("fp05_field_named_slot", '    on pure(id) { let m = map("acct", 4) return m.acct }\n', tests=['assert pure("k") == 4'])
case("fp06_ufcs_builtin_named_signal", '    on pure(id) { let xs = list(1, 2) return xs.peek2() }\n    on _len2(xs: List) { return 2 }\n', tests=['assert pure("k") == 2'], face='')
case("fp07_established_source", '    on pure(id) { return 1 }\n    on go(id) { if get_status(id) == "new" { transition(id, "paid") } return 1 }\n', face='        signal go(id: String) -> Int  moves new -> paid\n', tests=['assert go("k") == 1'])
case("fp08_status_in_helper", '    on pure(id) { return 1 }\n    on go(id) { let s = _st(id) if s == "new" { transition(id, "paid") } return 1 }\n    on _st(id: String) { return get_status(id) ?? "new" }\n', face='        signal go(id: String) -> Int  moves new -> paid\n', tests=['assert go("k") == 1'])
case("fp09_lambda_param_slot", '    on pure(id) { let xs = map(list(1, 2), aux => aux + 1) return len(xs) }\n', tests=['assert pure("k") == 2'])
case("fp10_print_sleep_now", '    on pure(id) { print("x") let t = len(id) return t }\n', tests=['assert pure("k") == 1'])
