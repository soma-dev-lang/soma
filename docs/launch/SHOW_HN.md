# Show HN draft

**Title (≤80 chars):**

> Show HN: Soma – a language where programs carry their proofs, built for AI agents

**Post body:**

I've been building Soma, an experimental language where verification is the
center of gravity rather than an add-on. Three ideas compose:

1. **State machines are proven.** Every lifecycle is model-checked
   (`soma verify`): reachability, deadlock, liveness — and a refinement
   check proves the handler *bodies* implement the machine they're declared
   next to, so the spec and the code can't drift.

2. **Memory carries invariants.** `invariant balance >= 0` is checked
   before any write commits. A violating write is rejected with the slot
   unchanged — an overdraft isn't a bug you catch, it's unrepresentable.

3. **LLMs run inside the cage.** An agent's lifecycle is a state machine
   the compiler proves terminates; `set_budget` hard-caps token spend; tool
   calls are capability-scoped. We tested it by giving an adversarial mock
   LLM a checkbook: it demands $999,999 on a $200 invoice, and the ledger
   pays $200 (the demo is in the repo: `treasury/`).

The repo has six complete apps where the safety property is a theorem you
can re-run — an airlock where "both doors open" is unreachable, a poker
server bots play over HTTP where chips provably can't leak, a market maker
whose kill switch can't re-arm — plus 168 verified example programs
generated specifically as LLM training data (every one passes check+test;
training on broken code teaches broken code).

It's honest-labeled experimental: per-cell proofs (cross-cell composition
is linted, not proven), an AST-walking interpreter (with `[native]` Rust
codegen for hot paths — measured at rustc -O parity), and a young package
registry (sparse HTTP index, Cargo-style lockfile, packages ship their
proofs and re-verify on your machine).

Everything in the README is a command you can run:
https://github.com/soma-dev-lang/soma

**Notes for posting:**
- Best window: Tue–Thu, 8–10am ET.
- First comment (yours): a short honest "what's NOT done" list — HN
  rewards it. Use the README's "Honest status" section.
- Expect: "why not TLA+/P/Erlang?" → wiki/synthesis/vs-* has the answers;
  the short version: those verify models *next to* the code; Soma verifies
  the code itself, and adds the LLM-containment layer none of them have.
