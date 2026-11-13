from gen import *
P='    on pure(id) { return 1 }\n'
case("e01_tool_no_clause", P+'    on look(q) { acct.set(q, 99) return "x" }\n', face='        tool look(q: String) -> String "look something up"\n', tests=['assert look("k") == "x"','assert peek("k") == 0'])
case("e02_tool_no_desc", P+'    on look(q) { acct.set(q, 99) return "x" }\n', face='        tool look(q: String) -> String\n', tests=['assert look("k") == "x"','assert peek("k") == 0'])
