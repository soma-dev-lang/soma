import os,subprocess,re
S="/private/tmp/claude-501/-Users-antoine-soma/42606278-6030-4a1b-bdf3-5eabb6b81862/scratchpad/r6/bin/soma"
TOML='[package]\nname = "v"\nedition = "2027"\n[verify.before."Pay.paid"]\nrequires = ["Approval.signed_off"]\n'
APPR='''skeleton Approval {
    state appr {
        initial: pending
        pending -> signed_off
    }
    face {
        signal sign(id: String) -> Int        moves pending -> signed_off
        signal status(id: String) -> String   reads appr
    }
}
flesh Approval {
    on sign(id) { transition(id, "pending", "signed_off")  return 1 }
    on status(id) { return get_status(id) ?? "pending" }
}
'''
TEST='''cell test T {
    rules {
        assert Pay.pay("k") == 1
        assert Pay.pst("k") == "paid"                   // paid…
        assert Approval.status("k") == "pending"        // …never signed off
    }
}
'''
def pay(edges, frame, body, extra_sig="", extra_fl=""):
    return f'''skeleton Pay {{
    state pay {{
        initial: due
{edges}
    }}
    face {{
        signal pay(id: String) -> Int        {frame}
        signal pst(id: String) -> String     reads pay
{extra_sig}    }}
}}
flesh Pay {{
    on pay(id) {{
{body}
    }}
    on pst(id) {{ return get_status(id) ?? "due" }}
{extra_fl}}}
'''
def w(name, src, toml=TOML, pat=r'before Pay'):
    os.makedirs(name,exist_ok=True); open(name+"/app.cell","w").write(src); open(name+"/soma.toml","w").write(toml)
    out=[]
    for rep in range(2):
        r=lambda *a: (lambda x:(x.returncode,x.stdout+x.stderr))(subprocess.run([S,*a,"app.cell"],cwd=name,capture_output=True,text=True,timeout=60))
        cc,co=r("check"); vc,vo=r("verify"); tc,to=r("test")
        lines=[l.strip()[:150] for l in vo.splitlines() if re.search(pat,l)]
        verdict=[l for l in vo.splitlines() if l.startswith("VERIFY")][-1]
        out.append(f"check rc={cc} errors={len([l for l in co.splitlines() if l.startswith('error')])} | {lines} | {verdict[:40]} | test rc={tc} {to.strip().splitlines()[-1]}")
    print(name, "\n   ", out[0], "\n    same twice:", out[0]==out[1])
E3="        due -> ready\n        ready -> paid\n        ready -> due"
w("V1_continue", APPR+pay(E3,"moves due -> ready, ready -> paid, ready -> due",
'''        for x in [1, 2] {
            if x == 1 { transition(id, "due", "ready")  continue }
            transition(id, "ready", "paid")              // second turn: ready -> paid, never signed
        }
        if get_status(id) == "ready" { transition(id, "ready", "due") }
        return 1''')+TEST)
w("V2_std_transition", APPR+pay("        due -> paid","moves due -> paid",
'''        std.transition(id, "due", "paid")
        return 1''')+TEST)
w("V3a_interpolated_id", APPR+pay("        due -> paid","moves due -> paid   calls Approval.sign",
'''        let x = "other"
        Approval.sign("{x}")                          // signs "other"
        x = id
        transition("{x}", "due", "paid")              // pays id: the same literal text, another value
        return 1''')+TEST)
w("V3b_interpolated_target", APPR+pay("        due -> paid\n        due -> void","moves * -> *",
'''        let t = "paid"
        transition(id, "{t}")
        return 1''')+TEST)
w("V4_states_differing_by_case", APPR+pay("        due -> Paid\n        due -> paid","moves due -> paid",
'''        transition(id, "due", "paid")
        return 1''')+TEST)
