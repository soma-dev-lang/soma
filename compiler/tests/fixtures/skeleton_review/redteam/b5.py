from gen import *
F='        signal rd(id: String) -> Int  reads acct\n'
P='    on pure(id) { return 1 }\n'
T=['assert bump("k") == 1','assert rd("k") != 12345','assert peek("k") == 1']
for i,body in enumerate(['let r = set(acct, id, 5)','let r = acct |> set(id, 5)','let r = put(acct, id, 5)','let r = delete(acct, id)','let r = remove(acct, id)','let r = clear(acct)','let r = append(acct, 5)','let m = acct\n m.set(id, 5)','let r = (acct).set(id, 5)','let r = acct.update(id, 5)','let r = acct.incr(id)','let r = acct.insert(id, 5)','let r = acct.pop(id)','let r = acct.merge(map(id, 5))']):
    case("d%02d_fnform"%i, P+'    on rd(id) { '+body+' return 1 }\n', face=F, tests=T)
