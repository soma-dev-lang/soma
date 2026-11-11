# Show HN draft

**Title (≤80 chars):**

> Show HN: Soma – a language where programs carry their proofs, built for AI agents

**Post body:**

I've been building Soma, an experimental language where verification is the
center of gravity rather than an add-on. Three ideas compose:

1. **State machines are proven.** Every lifecycle is model-checked
   (`soma verify`): reachability, deadlock, liveness. Where a handler's
   transition targets are literal, a refinement check proves the handler
   *body* implements the machine drawn next to it, so the spec and the code
   can't drift. Where a target is computed at run time — from input, or from
   an LLM's answer — the check falls back to a runtime one, and verify says
   so in its output instead of quietly counting it as proven.

2. **Memory carries invariants.** `invariant balance >= 0` is checked
   before any write commits. A violating write is rejected with the slot
   unchanged — an overdraft isn't a bug you catch, it's refused at the write.
   When the value comes from a parameter, verify labels that invariant
   runtime-checked rather than proven, and prints the `require` that would
   make it static.

3. **LLMs run inside the cage.** An agent's lifecycle is a state machine
   the compiler proves terminates; `set_budget` hard-caps token spend; tool
   calls are capability-scoped. We tested it by giving an adversarial mock
   LLM a checkbook: it answers $999,999 on a $200 invoice, and the ledger
   moves $200 — the model's number is advisory and can only ever lower the
   payment. `soma verify treasury/app.cell` prints `think-isolated: CTL
   safety properties hold regardless of LLM output (5 handlers, 5 literal
   transitions, 0 dynamic)`. The demo is in `treasury/`:

       soma run treasury/app.cell seed 1000
       soma run treasury/app.cell approve_vendor acme
       SOMA_LLM_MOCK='fixed:999999' \
         soma run treasury/app.cell process_invoice acme 200
       → {"claimed": 200, "model_proposed": 999999,
          "authorized_amount": 200, "balance": 800,
          "outcome": "paid $200 to acme"}

The repo has six complete apps you can re-run — an airlock where "both
doors open" is not a state the machine has, a poker server bots play over
HTTP where the chip total never moves, a market maker whose kill switch
can't re-arm — plus 321 verified example programs generated specifically as
LLM training data (every one passes check+test; training on broken code
teaches broken code).

It's honest-labeled experimental: per-cell proofs (cross-cell composition
is linted, not proven), an AST-walking interpreter (with `[native]` Rust
codegen for hot handlers; the benchmark harness and its C/Rust references
are in `bench/`), cluster replication that is eventual and experimental, and
a young package registry (sparse HTTP index, Cargo-style lockfile, packages
ship their proofs and re-verify on your machine). The exact scope of what is
and isn't proven: https://soma-lang.dev/docs/guarantees.md

Everything in the README is a command you can run:
https://github.com/soma-dev-lang/soma

**Notes for posting:**
- Run every command in this post from a FRESH clone the morning you post.
  Several demos keep `[persistent]` state, so a command that works in your
  working copy can behave differently on an empty `.soma_data/` — that is
  how the treasury and airlock write-ups were wrong until 2026-09-21.
- Best window: Tue–Thu, 8–10am ET.
- First comment (yours): a short honest "what's NOT done" list — HN
  rewards it. Use the README's "Honest status" section.
- Expect: "why not TLA+/P/Erlang?" → wiki/synthesis/vs-* has the answers;
  the short version: those verify models *next to* the code; Soma verifies
  the code itself, and adds the LLM-containment layer none of them have.
