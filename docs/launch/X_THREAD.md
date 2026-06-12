# X/Twitter thread draft

**1/**
We gave an adversarial LLM a checkbook.

It demanded $999,999 on a $200 invoice.
The account paid $200.

Not because the prompt said so — because the language made overpaying
unrepresentable. This is Soma. 🧵

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
✓ liveness: every state reaches a terminal
✓ invariant account >= 0 — writer 'seed' proven
```

That's not a test passing. That's a theorem about every execution.

**4/**
Six complete apps in the repo where safety is a theorem:

🏦 treasury — AI can't exceed its authority
🚪 airlock — "both doors open" is unreachable
🃏 poker — bots play over HTTP; chips provably can't leak
🛗 elevator — buggy scheduler, proven interlock
📉 market maker — kill switch provably can't re-arm

**5/**
It's also built FOR agents writing code:

- error messages contain their own fix
- `soma describe --builtins --json` = exact signatures, no guessing
- AGENT_GOTCHAS.md = verified wrong→right pairs
- 168 example programs, every one passing check+test — correct-by-
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
proven). Interpreter + [native] Rust codegen (measured at rustc -O parity).
Young package registry where packages ship their proofs and re-verify on
YOUR machine.

Everything in the README is a command you can run:
github.com/soma-dev-lang/soma
