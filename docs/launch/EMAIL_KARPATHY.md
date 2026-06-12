# Email draft — Andrej Karpathy

**To:** (his email — not publicly listed; X DM @karpathy is the realistic channel,
or the same text works as a DM/intro-thread)

**Subject:** A language where the verifier is the reward — agents wrote most of it

---

Hi Andrej,

Two ideas of yours kept resurfacing while I built this, so you should see
where they led.

Soma is a small experimental language designed for the agent loop rather
than the human one: state machines are model-checked (`soma verify`),
memory invariants reject illegal writes before they commit, and LLM
agents run inside lifecycles the compiler proves terminate, with hard
token budgets. The agent proposes; the verifier disposes. The demo that
captures it: an adversarial mock LLM gets a checkbook, demands $999,999
on a $200 invoice — and the ledger pays $200, because overpaying isn't
representable. (github.com/soma-dev-lang/soma → `treasury/`)

Two parts you may find more interesting than the language itself:

1. **The training corpus is correct by construction.** 300+ complete
   programs, every one gated through `check` + `test` before being kept —
   generated largely *by* agents, with the compiler as the reward signal.
   Training data for a new language that cannot teach hallucinations.

2. **Most of the recent compiler was built the same way**: an agent
   writing Rust, gated by the test suite plus an adversarial review
   fleet — ~30 releases in a week, including the verifier features that
   then gated its own later work. Verifier-in-the-loop, eaten as dogfood.

The docs follow your llms.txt / small-wiki patterns
(soma-lang.dev/llms.txt). If any of this is wrong-headed I'd genuinely
like to know where — that would be the most useful reply.

Antoine

---

**Notes:**
- ~230 words; he reads short emails. Resist adding more.
- No ask beyond "tell me where it's wrong" — the highest-response-rate
  ask for him.
- If DM instead: drop the greeting, lead with the checkbook story.
- Every claim is verifiable in the repo; if he pulls one thread it holds.
