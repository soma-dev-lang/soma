# Soma Vision: The Future of Agent Programming

*April 2026 -- VISION report*

## Where Soma Is Now

Soma has already made three bets that most languages haven't even considered:

1. **The compiler is the supervisor.** Agents write code; the compiler verifies contracts, temporal properties, and distribution consistency before anything runs. This is not linting -- it is formal verification of state machine behavior and CAP theorem compliance at compile time. No other executable language does this.

2. **The cell is fractal.** The same construct -- memory, state machine, signal handlers, face -- describes a function, a service, a database, and a distributed cluster. There is no YAML, no Dockerfile, no Helm chart. The gap between "what the code says" and "what the infrastructure does" is zero.

3. **The agent workflow is a first-class loop.** Generate, check, verify, serve. The MCP tools (`soma_generate`, `soma_check`, `soma_verify`, `soma_serve`) give agents a tight feedback cycle with machine-readable errors. The `soma describe` command emits structured JSON so agents can introspect running systems.

These are genuine innovations. The cell model is more expressive than Kubernetes YAML while being formally verifiable. The verification pipeline catches classes of bugs that testing cannot reach (cycles in state machines, CAP contradictions, contract violations). The fractal uniformity means there is exactly one mental model at every scale.

But Soma is still a language that agents *use*. The vision is a language that agents *inhabit*.

---

## Five Big Bets

### 1. Intent Compilation: From "What I Want" to Verified Cells

**The problem.** Today, an agent reads AGENT.md, memorizes syntax rules, generates a `.cell` file, and hopes `soma check` passes. The agent is doing manual translation from intent to syntax. Every syntax mistake ("use `list()` not `[]`", "use `on` not `function`") is a wasted round-trip.

**The bet.** Add an intent layer above the cell language. An agent writes:

```
intent OrderSystem {
    "Users place orders. Orders go through approval, then fulfillment.
     Orders can be cancelled before fulfillment. Fulfilled orders are final.
     All order data must survive restarts. The system serves HTTP on port 8080."
}
```

The Soma compiler itself compiles intent to cells -- generating the state machine, memory declarations, handlers, and face contracts -- then immediately verifies the result. The agent gets back either a working cell or a structured explanation of why the intent is ambiguous ("'approval' -- is this a single step or does it require multiple approvers?").

**What it enables.** Agents stop being syntax translators and become system designers. The feedback loop becomes: express intent, resolve ambiguities, verify properties. The compiler does the mechanical work.

**Design sketch.** The intent compiler is a constrained natural-language-to-AST pass that targets the existing cell model. It does not need to be general-purpose NLP -- it needs to understand a fixed vocabulary: entities (nouns become memory slots), lifecycles (verb sequences become state machines), durability ("must survive" implies `[persistent, consistent]`), and exposure ("serves HTTP" implies `on request`). Ambiguities produce structured questions, not errors. The output is a standard `.cell` file that the agent can inspect, modify, and re-verify.

**Why it matters.** The best language for agents is one where agents never fight syntax. Soma's cell model is regular enough that intent-to-cell compilation is tractable -- the target has only five sections (face, memory, state, scale, handlers), not the combinatorial explosion of a general-purpose language.

---

### 2. Diagnostic Agents: Self-Healing Through Structured Error Repair Plans

**The problem.** When `soma check` fails, the agent gets an error message and has to figure out the fix. When `soma verify` produces a counter-example trace (`shipped -> failed -> pending -> confirmed -> shipped -> ...cycle`), the agent must reason about graph theory to find the repair. This is doable but brittle -- agents sometimes make the wrong fix and spiral.

**The bet.** Every diagnostic should include a **repair plan**: a structured, machine-readable description of exactly what to change and why.

```json
{
  "error": "eventually(delivered) unprovable",
  "counter_example": ["shipped", "failed", "pending", "confirmed", "shipped"],
  "diagnosis": "cycle through 'failed -> pending' allows infinite retry without reaching 'delivered'",
  "repair_options": [
    {
      "strategy": "add_terminal",
      "description": "Add 'failed_permanent' state with transition 'failed -> failed_permanent' and make delivered/failed_permanent/cancelled the terminal set",
      "patch": "state trade { ... failed -> failed_permanent ... }",
      "new_property": "eventually = [\"delivered\", \"failed_permanent\", \"cancelled\"]"
    },
    {
      "strategy": "break_cycle",
      "description": "Remove 'failed -> pending' transition to eliminate the retry cycle",
      "patch": "// remove: failed -> pending"
    }
  ]
}
```

**What it enables.** Zero-shot error repair. The agent does not need to understand model checking theory -- it picks a repair strategy, applies the patch, and re-verifies. The compiler becomes a collaborator, not just a gatekeeper.

**Design sketch.** The model checker already computes counter-example traces. Extending it to compute repair plans requires cycle detection (for liveness failures), reachability analysis (for deadlock), and contradiction detection (for property conflicts). Each failure class maps to a small set of known repair strategies. The repair plan generator is a post-pass on the existing verification output.

**Why it matters.** Agent reliability is bottlenecked by error recovery, not error detection. A language that tells you what's wrong and how to fix it is exponentially more agent-friendly than one that only tells you what's wrong.

---

### 3. Cell Composition Protocol: A Package Manager for Verified Components

**The problem.** Soma has `use lib::module` for imports and `interior {}` for nesting cells. But there is no standard way for one agent to publish a verified cell that another agent can safely compose into a larger system. The `self_growing.cell` example shows custom properties, types, and checkers -- powerful primitives -- but there is no registry, no versioning, no composition verification.

**The bet.** Build a composition protocol where cells are published with their face contracts and verification proofs, and the compiler checks that composed cells satisfy each other's contracts at integration time.

```
use registry::payments/Gateway@2.1    // verified cell from registry
use registry::notifications/Notifier@1.0

cell MySystem {
    interior {
        Gateway { }     // face contract checked against MySystem's usage
        Notifier { }    // await/emit graph verified across composition
    }
    runtime {
        connect Gateway.payment_processed -> Notifier
    }
}
```

When `soma check` runs on this file, it verifies: (a) every `await` in Notifier has a matching `emit` somewhere in the composition, (b) signal types match across cell boundaries, (c) memory property requirements are compatible, (d) the combined state machine graph satisfies the composed temporal properties.

**What it enables.** Agent specialization. One agent builds and verifies a payment cell. Another builds a notification cell. A third composes them. Each agent works in its domain; the compiler ensures the composition is sound. This is how agent ecosystems actually scale.

**Design sketch.** Published cells include: source code, face contract (the cell's API), verification certificate (which temporal properties were proven), and dependency graph. The registry is content-addressed (like Unison) so that a cell's identity is its verified behavior, not its name. Composition checking extends the existing signal checker to work across cell boundaries, which it partially already does for interior cells.

**Why it matters.** No agent will build everything from scratch. The ability to compose verified components is the difference between "agents can write programs" and "agents can build systems."

---

### 4. Live Verification: Continuous Proof Maintenance in Running Systems

**The problem.** Verification happens once, at compile time. But systems evolve. An agent modifies a handler, adds a state, changes a memory property. The verified properties may no longer hold, but the agent won't know until it explicitly re-runs `soma verify`. In a multi-agent scenario, Agent A modifies a cell while Agent B's code depends on properties that Agent A just broke.

**The bet.** Make verification incremental and continuous. Every change triggers re-verification of affected properties. The system maintains a "proof status" that is always current.

```
$ soma serve app.cell --watch --verify

[14:23:01] Serving on :8080
[14:23:15] File changed: app.cell
[14:23:15] Re-checking... passed (12ms)
[14:23:15] Re-verifying... 
  ✓ deadlock_free (cached — state machine unchanged)
  ✗ eventually(settled) — BROKEN by new transition 'failed -> retry'
  → Repair: add terminal state or bound retry count
[14:23:15] WARNING: serving with unverified properties
```

**What it enables.** Agents can iterate on running systems with continuous safety feedback. Multi-agent teams get immediate notification when one agent's change breaks another's invariants. The "generate-check-verify-serve" loop collapses into a single continuous process.

**Design sketch.** The incremental verifier tracks which AST sections changed and which properties depend on them. State machine changes invalidate temporal properties. Memory property changes invalidate distribution checks. Handler body changes invalidate nothing at the verification level (they are below the abstraction). The existing model checker runs in under 100ms, so re-verification on every save is feasible.

**Why it matters.** Static verification is a gate. Continuous verification is a guardrail. Gates slow agents down; guardrails keep them safe while they move fast.

---

### 5. Behavioral Reflection: Cells That Describe Themselves to Agents

**The problem.** `soma describe` emits JSON about a cell's structure: handlers, memory, state machines. But it says nothing about behavior -- what the handlers actually do, what data flows where, what the system's current state is. An agent inheriting a system must read the source code to understand it.

**The bet.** Extend describe to produce behavioral summaries: data flow graphs, state machine current state, handler dependency chains, and natural-language explanations generated from the AST.

```json
{
  "cell": "TradingDesk",
  "behavior": {
    "data_flow": [
      "request -> _route -> execute_trade -> trades.set",
      "every 60s -> http_get -> price_history.set"
    ],
    "state_machine": {
      "name": "trade_status",
      "current_distribution": {"pending": 12, "filled": 45, "settled": 203},
      "bottleneck": "filled (avg 4.2s dwell time)"
    },
    "invariants_holding": ["deadlock_free", "eventually(settled|cancelled)"],
    "capabilities": "HTTP API with 14 endpoints. Persistent storage across 5 slots. Scheduled price fetching every 60s.",
    "modification_risks": [
      "Adding states to trade_status may break eventually(settled|cancelled)",
      "Changing trades memory properties requires re-verification of scale section"
    ]
  }
}
```

**What it enables.** Agent handoff. Agent A builds a system. Agent B takes over maintenance. B calls `soma describe --behavior app.cell` and gets a complete understanding without reading source code. This is also the foundation for agents that monitor and optimize running systems -- they need behavioral understanding, not just structural description.

**Design sketch.** Data flow analysis walks the AST of each handler, tracking which memory slots are read/written and which signals are emitted. State machine runtime stats come from instrumenting transitions (a counter per edge). Modification risk analysis is the inverse of incremental verification: "if you change X, properties Y and Z need re-verification." Natural-language summaries are template-generated from the structural analysis, not LLM-generated -- they must be deterministic and trustworthy.

**Why it matters.** The bottleneck in agent programming is not writing code -- it is understanding existing systems well enough to modify them safely. A language that makes its own programs legible to agents, without requiring source code reading, is a language where agent teams can actually collaborate on long-lived systems.

---

## The Through-Line

These five bets share a single thesis: **the compiler should do for agents what IDEs did for humans, but better.** IDEs gave humans syntax highlighting, autocomplete, and error squiggles. Soma should give agents intent compilation, repair plans, composition verification, continuous proofs, and behavioral reflection.

The cell model is the right foundation. It is regular (five sections), verifiable (state machines + contracts + temporal logic), composable (interior cells + signal bus), and introspectable (structured AST). Every bet above is an extension of what the cell model already makes possible.

The end state: an agent says what it wants, the compiler builds and verifies it, other agents compose and extend it, the system monitors its own invariants, and any agent can understand any system without reading its source. That is not a programming language. It is a programming environment where agents are native citizens.
