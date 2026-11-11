# X/Twitter thread draft

**1/**
We gave an adversarial LLM a checkbook.

It demanded $999,999 on a $200 invoice.
The account paid $200.

Not because the prompt said so — the model's number is advisory, and the
code can only ever lower the payment with it. This is Soma. 🧵

**2/**
Soma is a language where programs carry their proofs:

- state machines are model-checked (`soma verify`)
- memory invariants are enforced BEFORE writes commit
- LLM agents run inside lifecycles the compiler proves terminate,
  with hard token caps

The model proposes. The language disposes.

**3/**
```
$ soma verify treasury/app.cell
✓ terminal states: [paid, rejected]
✓ liveness: every state can eventually reach a terminal state
✓ think-isolated: CTL safety properties hold regardless of LLM output
  (5 handlers, 5 literal transitions, 0 dynamic)
✓ refinement: handler `settle` ⟶ {paid}
```

That last line is the one that matters: the machine's safety doesn't depend
on what the model answers. (The balance invariant takes its amount from a
parameter, so verify labels that one runtime-checked, not proven — it prints
exactly that, and prints the `require` that would make it static.)

**4/**
Six complete apps in the repo where safety is a theorem:

🏦 treasury — AI can't exceed its authority
🚪 airlock — "both doors open" isn't a state the machine has
🃏 poker — bots play over HTTP; chips provably can't leak
🛗 elevator — buggy scheduler, proven interlock
📉 market maker — kill switch provably can't re-arm

**5/**
It's also built FOR agents writing code:

- error messages contain their own fix
- `soma describe --builtins --json` = exact signatures, no guessing
- AGENT_GOTCHAS.md = verified wrong→right pairs
- 321 example programs, every one passing check+test — correct-by-
  construction LLM training data

**6/**
And it reads like numpy where you want it to:

```
let M = [1,0,0,1].reshape(2,2)
A * B          // matmul
A + 10         // broadcast
v * v          // elementwise
A > 2          // masks
```

**7/**
Honest status: experimental. Per-cell proofs (cross-cell is linted, not
proven). A transition target computed at run time is checked at run time,
and verify says so. Cluster replication is eventual. Interpreter + [native]
Rust codegen for hot handlers. Young package registry where packages ship
their proofs and re-verify on YOUR machine.

Everything in the README is a command you can run:
github.com/soma-dev-lang/soma
