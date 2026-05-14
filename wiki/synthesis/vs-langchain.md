---
name: vs-langchain
description: Comparison with agent frameworks (LangChain, CrewAI, AutoGen).
type: synthesis
since: V1.0
related: [think, think-isolation, budget-proof, manifesto]
---

# Soma vs LangChain (and other agent frameworks)

LangChain, CrewAI, AutoGen, and OpenAI Assistants are the dominant
agent frameworks. They're libraries on top of a host language
(Python/JS). Soma takes a different bet: **agents are a kind of
program; the language should reason about them**.

This page compares Soma against agent frameworks specifically. For
general distributed systems comparisons see [[vs-erlang-pony]] and
[[vs-kubernetes]].

## What's the same

- **Tool calling.** Soma's [[face]] declares tools; LangChain has
  `@tool` decorators. Both produce JSON schemas that get fed to the
  LLM.
- **Memory.** Soma's `remember()` / `recall()` is analogous to
  LangChain's memory module.
- **Multi-agent.** Soma uses `interior {}` cells; LangChain has
  Crews, AutoGen has GroupChats. Same idea, different syntax.

## What's different

### 1. Termination is **proven**

LangChain: a `while True: think()` loop can run forever and there's
no compiler check. The author writes it and hopes.

Soma: every handler is proven to terminate by the [[termination]]
checker. Unbounded loops require explicit `[loop_bound(N)]` or are
rejected.

### 2. Budget is **proven**

LangChain: token budget is enforced by the LLM provider (rate limits,
`max_tokens` per call). Memory pressure of intermediate values is
untracked.

Soma: [[budget-proof]] proves peak memory ≤ declared bound at compile
time. `think("...", map("max_tokens", 500))` contributes a closed-form
cost; the cell budget aggregates them.

```soma
cell agent Researcher {
    scale { memory: "256Mi" }            // ← compile-time bound
    on research(topic: String) {
        let r = think("Research {topic}", map("max_tokens", 3000))
        ...
    }
}
// soma check: ✓ budget proven: peak ≤ 24.34 MiB ≤ 256.00 MiB
```

### 3. State-machine safety holds under adversarial LLM

LangChain: the agent's state is whatever the Python code does with
the LLM's output. There's no theorem about safety.

Soma: [[think-isolation]] is a mechanical theorem. CTL safety
properties of the state machine hold **regardless of what the LLM
returns**. The Coq proof in `Soma_Isolation.v` treats the LLM as an
adversary.

```soma
on classify(order: Map) {
    let label = think("Classify: {order}")
    transition("inst", "classified")    // literal — isolated
    label
}
```

The LLM cannot make `transition("inst", "deleted_account")` happen.
The target is a literal compiled-in string.

### 4. Compile-time contracts

LangChain: handler-to-handler calls are Python function calls;
type-check at best via mypy, no contract enforcement.

Soma: [[face]] contracts are compile-checked. Cross-cell `delegate`
calls go through declared signals with arity-matched parameters.

### 5. Distribution model

LangChain: deployment is "ship your Python on a server." Sharding,
replication, consensus are out of scope.

Soma: [[scale]] is part of the cell. The same source runs standalone
or as a 50-replica cluster.

## What LangChain has that Soma doesn't

Be honest:

- **Maturity.** LangChain has thousands of integrations, vector
  stores, RAG primitives. Soma has the basics + `recall_similar`.
- **Ecosystem.** LangChain plugs into LlamaIndex, Pinecone, every
  vector DB. Soma's `recall_similar` is provider-specific.
- **Streaming.** LangChain has streaming responses; Soma's
  `think()` is synchronous.
- **Multi-modal.** LangChain handles images/audio/video. Soma is
  text-first.
- **Examples / tutorials / docs.** LangChain has thousands of how-to
  guides. Soma has [[verification-overview]] and this wiki.

## Where Soma wins

For systems where any of these matter:

- **Regulated environments** (finance, healthcare): "I have a
  mechanical proof the agent cannot enter a forbidden state" is
  load-bearing.
- **Long-running pipelines**: termination + budget proofs eliminate
  whole classes of "the agent ran for 6 hours and racked up $400"
  bug.
- **Multi-agent systems with critical handoffs**: state-machine
  refinement makes the orchestration verifiable.
- **Provable preconditions**: `ensure imp.bps <= 30` is something no
  LangChain agent can promise.

## Where LangChain wins

- **Prototyping speed** when the goal is "talk to a vector DB."
- **Integration breadth** when you need an existing service Soma
  hasn't wrapped.
- **Streaming UX** for chat applications.
- **Ecosystem effects** — finding answers on Stack Overflow.

## Migration guide

Moving from LangChain to Soma:

1. **Chain → cell.** A LangChain `Chain` maps to a `cell agent`.
2. **Tools → face.** `@tool` decorators become face declarations
   plus `on handler_name` bodies.
3. **Memory → `remember`/`recall`**. Same idea, different API.
4. **Crews → interior cells.** A multi-agent Crew is a parent cell
   with `interior { cell agent A …  cell agent B … }`.
5. **Add a state machine.** The implicit Python control flow becomes
   an explicit `state { … }` block. This is where the verification
   wins are.
6. **Run `soma check` + `soma verify`.** Iterate until proven.

Expect step 5 to be the hardest. Most LangChain code has *implicit*
state; making it explicit is a refactor, but the resulting code is
verifiable.

## Honest cut

For **chat apps and one-off prototypes**, LangChain is simpler.

For **production agent systems with safety / compliance / budget
requirements**, Soma's verification story is irreplaceable. No
LangChain agent can promise its budget, its termination, or its
state-machine safety the way a verified Soma cell can.

The bet: as agent systems get more critical, the value of these
proofs grows.

## Related

- [[think]] — the bounded LLM call.
- [[think-isolation]] — the safety theorem.
- [[budget-proof]] — proven memory consumption.
- [[manifesto]] — the broader thesis.
