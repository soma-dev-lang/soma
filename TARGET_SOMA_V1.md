# TARGET_SOMA_V1

The path from "interesting language" to "wow language."
Five features, one rigor pass, one war story.

---

## 0. The rigor pass (non-negotiable foundation)

Before any new feature ships, Soma v1 needs the spine its manifesto already
implies but does not yet possess:

- **Small-step operational semantics** for the cell calculus, on paper.
- **Soundness theorem** for the CTL model checker against that semantics.
- **Backend equivalence:** `interpreter ≡ bytecode ≡ [native]` as an
  observational equivalence (or refinement) theorem. Without this, the 300×
  speedup might be 300× the wrong answer.
- **Adversary-quantified statements** of every existing guarantee:
  "deadlock-free *under what network model*?"
- **State-explosion honesty:** publish the largest state space `soma verify`
  handles in practice; document the cliff.

This is the unglamorous work that turns the manifesto from a vibe into a
theorem. Everything below assumes it lands first.

---

## 1. `prove` — proof-carrying verification

Exportable, third-party-replayable proof witnesses.

```soma
state payment {
    initial: pending
    pending → authorized → captured → settled
    * → refunded
}

prove payment {
    invariant: captured ⇒ ◇ settled ∨ ◇ refunded
    export: lean4 → "proofs/payment.lean"
}
```

`soma verify --export-proof` emits a Lean4 (or Coq) file that any third party
can replay without trusting the Soma binary. Breaks the "trust the verifier"
circle. CompCert-grade credibility in one annotation.

**Why it matters:** no other practical language ships exportable proof
witnesses today. Soma would be the first.

---

## 2. Session-typed `signal` — protocols enforced end-to-end

```soma
protocol auction {
    bidder → seller : Bid(amount: Int)
    seller → bidder : Ack | Reject
    loop {
        bidder → seller : Raise(amount: Int)
        seller → bidder : Ack | Reject | Close(winner: Id)
    }
}

cell bidder uses auction as bidder { ... }
cell seller uses auction as seller { ... }
```

The compiler refuses to deploy a cell whose handlers don't exhaust the
protocol, send messages out of order, or drop a branch.
**Deadlock-by-construction**, not deadlock-by-model-checking. Honda/Yoshida
session types, finally in a real production language.

**Why it matters:** kills a whole class of distributed bugs at compile time
with zero state explosion — *and it composes along cell boundaries*, which
patches the biggest hole in the current verification story.

**Ship priority: FIRST.** Smallest theory, largest bug prevention, makes the
fractal claim true.

---

## 3. `causal` memory — Lamport clocks as a type

```soma
memory {
    orders:    Map[OrderId, Order] [persistent, causal]
    inventory: Map[Sku, Int]       [persistent, causal]
}

on place_order(o: Order) {
    orders[o.id] = o          // tagged with vector clock
    inventory[o.sku] -= 1     // happens-after, enforced by the type system
}
```

Every read returns a value plus its causal context; every write extends it.
The compiler tracks happens-before in the type system and rejects code that
observes effects out of causal order. CRDTs become an implementation detail
of `[causal]`, not a library import.

**Why it matters:** Lamport's deepest idea, first-class in a type system.

---

## 4. `adversary` — declarative threat models

```soma
adversary network {
    drop:      up to 30% of messages
    reorder:   arbitrary
    delay:     bounded(5s)
    partition: any minority subset
}

adversary llm {
    output: arbitrary string
    rate:   bounded(set_budget)
}

scale {
    replicas: 5
    consistency: strong
    survives: network ∧ llm
}
```

`soma verify` proves liveness and safety **under the declared adversary**,
not under the assumption of a perfect world. The LLM adversary is the honest
answer to "what if the agent hallucinates" — currently nobody has one.

**Why it matters:** turns "Soma is verified" from a vibe into a theorem with
an explicit quantifier.

---

## 5. `replay` — deterministic time-travel as a primitive

```soma
cell trader {
    on tick(price: Float) [record] { ... }
}
```

```
$ soma replay trader.somalog --at "2026-04-08T14:32:11Z"
> state: regime=risk_on, position=412
> next signal: tick(price=187.42)
> step → ...
> divergence at tick #8451:
>     recorded: position=413
>     replayed: position=412
>     cause:    nondeterminism in handler `score` (uses now())
>     fix:      mark `score` [pure] or pass clock as input
```

Every `[record]` cell logs its inputs; `soma replay` re-executes bit-exactly
and the compiler flags any source of nondeterminism as a divergence point
*with a suggested fix*. Erlang dreamed of this; pure-by-default cells make
it achievable.

**Why it matters:** turns production incidents from forensics into
single-stepping. The "I replayed yesterday's outage on my laptop" moment
that makes engineers never go back.

---

## The pattern

Every feature above takes a primitive Soma already has (`signal`, `memory`,
`state`, `verify`, `[native]`) and **promotes it from an annotation into a
theorem**. None are bolt-ons; all deepen the manifesto's own claims.

## Ship order

1. **Rigor pass** (semantics, soundness, backend equivalence). Two months.
2. **Session-typed `signal`** (#2). Highest leverage, smallest theory.
3. **`replay`** (#5). Cheapest to build, biggest demo moment.
4. **`prove` export to Lean4** (#1). Credibility multiplier.
5. **`adversary` blocks** (#4). Quantifies everything that came before.
6. **`causal` memory** (#3). The crown jewel; ship last because it touches
   storage, types, and the runtime simultaneously.

## The war story

None of this matters without **one production system**, run by people who
are not the Soma team, where `soma verify` caught a bug a senior engineer
missed. TLA+ became real with DynamoDB. CompCert became real with Airbus.
Rust became real with Stylo. Find Soma's equivalent and put it on the front
page. Replace "300 lines vs 50,000" with "this is the bug we caught."

## The single sentence

> Soma v1 is the language where every annotation is a theorem,
> every protocol is a type, every replay is deterministic,
> and every guarantee names its adversary.
