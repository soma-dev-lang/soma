#!/usr/bin/env python3
# generates one dir per case: <name>/app.cell ; runs check + test with trace
import os, subprocess, sys, shutil
SOMA="/Users/antoine/soma/compiler/target/release/soma"
ROOT=os.path.dirname(os.path.abspath(__file__))
SK='''skeleton Bank {
    memory {
        acct: Map<String, Int> [persistent]
        aux: Map<String, Int> [persistent]
    }
    state order {
        initial: new
        new -> paid
        paid -> shipped
    }
    face {
        signal peek(id: String) -> Int      reads acct
        signal bump(id: String) -> Int      writes acct
        signal pay(id: String) -> String    moves new -> paid
        signal status(id: String) -> String reads order
        signal pure(id: String) -> Int
%FACE%
    }
}
'''
BASE='''    on peek(id) { return acct.get(id) ?? 0 }
    on bump(id) { acct.set(id, (acct.get(id) ?? 0) + 1) return 1 }
    on pay(id) { transition(id, "new", "paid") return "paid" }
    on status(id) { return get_status(id) ?? "new" }
'''
def case(name, flesh, face="", extra="", tests=None, pre=""):
    d=os.path.join(ROOT,name); shutil.rmtree(d,ignore_errors=True); os.makedirs(d)
    tests = tests or ['assert pure("k") != 12345', 'assert peek("k") == 0', 'assert status("k") == "new"']
    src = pre + SK.replace("%FACE%",face) + "\nflesh Bank {\n" + BASE + flesh + "\n}\n" + extra + "\ncell test T {\n    rules {\n" + "".join("        "+t+"\n" for t in tests) + "    }\n}\n"
    open(os.path.join(d,"app.cell"),"w").write(src)
    return run(name)
def run(name, file="app.cell"):
    d=os.path.join(ROOT,name)
    env=dict(os.environ, NO_COLOR="1", SOMA_LLM_MOCK="fixed:ok")
    c=subprocess.run([SOMA,"check",file],cwd=d,env=env,capture_output=True,text=True)
    tr=os.path.join(d,"trace.tsv")
    if os.path.exists(tr): os.remove(tr)
    t=subprocess.run([SOMA,"test",file],cwd=d,env=dict(env,SOMA_EFFECT_TRACE=tr),capture_output=True,text=True,timeout=60)
    out=(c.stdout+c.stderr).strip().splitlines()
    errs=[l for l in out if l.startswith("error") or "code: E" in l]
    print("=====",name,"check rc",c.returncode, "" if c.returncode else "PASS")
    for l in errs[:3]: print("  C|",l[:260])
    tout=(t.stdout+t.stderr).strip().splitlines()
    if c.returncode==0:
        for l in tout:
            if "FAILED" in l or "tests:" in l or "left:" in l or "rror" in l: print("  T|",l[:200])
        if os.path.exists(tr):
            print("  X|"," ; ".join(l.rstrip().replace("\t","|") for l in open(tr)))
