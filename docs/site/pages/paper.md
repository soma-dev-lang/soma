---
title: Scale as a type
full_title: Scale as a type — what a compiler can check about distribution, and what it cannot (Soma working paper, revised)
description: The Soma working paper, revised against the toolchain. Distribution requirements written in the source next to the data, what soma check and soma verify establish about them, and the claims of the first version that are withdrawn. Every output on the page is printed by the soma binary.
type: ScholarlyArticle
toc: yes
---

# Scale as a type: what a compiler can check about distribution, and what it cannot

<p class="lede">Soma working paper, 2026 — revised. The first version of this paper claimed more than the implementation delivers. This revision keeps the idea, states only what the toolchain does, and lists what is withdrawn. Every program on this page is compiled, and every output printed, by the <code>soma</code> binary when the site is built.</p>

## Abstract

In most systems the application and its deployment are two artefacts with no formal relation: code in one language, replication and resources in manifests the compiler never reads. Soma writes both in one place. A *cell* declares its state (`memory`), its lifecycle (`state`), its contract (`face`) and, in a `scale` section, what it asks of distribution: which slot is replicated, at what consistency, within what memory.

The question of this paper is narrow: **which of those declarations can a compiler hold a program to, before anything runs?** The answer, today, is: the coherence of the declarations with each other and with the data, the lifecycle of the cell, and a bound on memory. It is not the behaviour of a running cluster. Soma's cluster mode is an experimental, eventually consistent replication; the verifier says so in its own output, and refuses the declarations the runtime cannot honour.

## 1. The cell

One unit holds the contract, the data, the lifecycle and the behaviour. The same form describes a function, a service with HTTP routes, and a replicated store.

```soma file=engine.cell
cell Engine {
    face {
        signal book(id: String) -> String
        signal settle(id: String) -> String
        signal cancel(id: String) -> String
    }

    memory {
        trades: Map<String, String> [persistent]       // durable, replicated below
        cache: Map<String, String> [ephemeral, local]  // this node only
    }

    state trade {
        initial: queued
        queued -> executed
        queued -> cancelled
        executed -> settled
    }

    scale {
        replicas: 3
        shard: trades
        consistency: eventual
        tolerance: 1
        memory: "8Gi"
    }

    on book(id: String) {
        trades.set(id, "booked")
        transition(id, "queued", "executed")
        return get_status(id)
    }

    on settle(id: String) {
        transition(id, "executed", "settled")
        return get_status(id)
    }

    on cancel(id: String) {
        transition(id, "queued", "cancelled")
        return get_status(id)
    }
}
```

## 2. What is checked before the program runs

### 2.1 Properties of memory that cannot hold together

A slot's properties are declarations of intent, and two intents can contradict each other. That is a compile error, the way a type error is:

```soma file=contradiction.cell
cell Bad {
    memory { data: Map<String, Int> [persistent, ephemeral] }
    on f() { return 1 }
}
```

```run
!soma check contradiction.cell
```

### 2.2 A `scale` section that names what exists

The slot a `scale` section replicates must be a slot of the cell:

```soma file=noslot.cell
cell Store {
    memory { rows: Map<String, String> [persistent] }
    scale { replicas: 2  shard: orders  consistency: eventual }
    on put(k: String, v: String) { rows.set(k, v) }
}
```

```run
!soma check noslot.cell
```

### 2.3 A memory budget, proven or refused

`memory: "32Mi"` in `scale` is a bound the checker computes against: the declared capacity of each slot, the largest handler, the state machines and the runtime. A cell that cannot fit is refused; one that fits is told under which assumptions.

```soma file=tight.cell
cell Tight {
    memory { rows: Map<String, String> [persistent, capacity(100000)] }
    scale { memory: "32Mi" }
    on put(k: String, v: String) { rows.set(k, v) }
}
```

```run
!soma check tight.cell
soma check engine.cell
```

The bound holds *while the slots stay within their assumed sizes*: the assumption is printed, and it is not enforced at run time.

### 2.4 The lifecycle

The state machine of the cell is model-checked: every state reachable, no deadlock, every state able to reach a terminal one, and the handlers take only the edges the machine declares. Temporal properties (`eventually`, `never`, precedence) are stated in `soma.toml`. This part is a proof, for every input, and it is the same whether the cell runs on one node or several — which is also its limit: it is a proof about one cell's graph, not about a cluster.

### 2.5 What the verifier says about distribution

```run
soma verify engine.cell
```

Read the `Engine/scale` lines as they are written. `replicas` is a request that `soma serve` does not provision. `tolerance` is a declaration with no fault-tolerance proof behind it. The replication of `trades` is reported as **unproven distributed behaviour**, with its actual semantics: eventual, full replication, local reads that can be stale, concurrent writes that can overwrite each other. Under `--strict` that warning is a failure:

```run
!soma verify engine.cell --strict --summary
```

### 2.6 Declarations the runtime cannot honour are refused

`consistency: strong` would need consensus; `causal` would need a causal protocol. Neither exists in the runtime, so neither is accepted:

```soma file=strong.cell
cell Ledger {
    memory { entries: Map<String, Int> [persistent] }
    scale { replicas: 3  shard: entries  consistency: strong }
    on put(k: String, v: Int) { entries.set(k, v) }
}
```

```run
!soma verify strong.cell
```

This is the smallest honest form of "scale as a type": a declaration is either checked, reported as unproven, or refused. It is never silently accepted and not delivered.

## 3. The runtime, as it is

A node joins a cluster with one flag: `soma serve app.cell -p 8090 --join host:port`. What follows is in [the cluster guide](/guide/cluster), and in short:

- **Full replication, not sharding.** Despite its name, `shard: trades` selects a mutable Map that every connected replica eventually holds. Entries are not partitioned between nodes.
- **Eventual consistency by logical versions.** Each key carries a (Lamport counter, node id) version; the higher wins, deletions included. This is a deterministic conflict rule, not wall-clock order. Concurrent read-modify-write on two nodes can lose an update.
- **No quorum, no consensus, no cross-node transaction, no linearizable read.** Reads are local.
- **Advisory leadership for scheduled work.** The lowest live node id runs `every` blocks; a partition can produce two. There is no fencing.
- **Small clusters.** A full mesh with periodic state exchange; no demonstrated large-cluster throughput.

Within one node the guarantees are stronger and are the subject of [Guarantees](/guide/guarantees): a handler is atomic, an invariant is checked before every write, a transition to an undeclared edge is refused.

## 4. What this revision withdraws

The first version of this paper is corrected on these points.

| The first version said | What is true |
|---|---|
| The compiler performs a CAP analysis and verifies quorum arithmetic (26 of 50). | It does not. `tolerance` and `replicas` are declarations; the verifier says so. |
| `consistency: strong` is "declared and verified". | It is refused: the runtime has no consensus. |
| Keys are distributed over a consistent hash ring; each node stores its range. | Selected slots are fully replicated to every node. |
| The same program runs "on one machine or a thousand". | Cluster mode is experimental and meant for a few nodes. |
| Three applications with 5, 10 and 50 replicas were evaluated. | No evaluation at those sizes exists. The regression tests start a handful of real processes on one machine. |
| Soma is the first language to verify distribution requirements at compile time. | Withdrawn. What it checks is the coherence of declarations; other systems go further on the protocol itself. |
| The code samples. | Several did not compile with the released toolchain. Every sample here does, at build time. |

## 5. Related work

**Erlang/OTP** gives transparent distribution of actors, without compile-time checks of what is asked of it. **TLA+** and **P** specify and model-check distributed protocols — far beyond what is checked here — as artefacts separate from the running code. **Kubernetes** manifests state replication and resources outside the application's language. **Orleans** and **Akka** place actors and configure consistency at the level of a library. **Unison** distributes content-addressed code. Verified implementations of consensus (**IronFleet**, **Verdi**) prove the property this paper only declines to claim.

Soma's contribution is smaller and of a different kind: the declarations sit in the source next to the data they speak of, the compiler holds them to each other, and the tool's output separates what is proven, what is checked at run time, and what is not covered.

## 6. Where the language went

Distribution was the first question Soma asked. The one it now answers best is another: how a program written by coding agents can be held to what was decided before the code. The same discipline — write the intent in the source, check what can be checked, say what cannot — became the skeleton and its frames, and then the law: business rules written outside the code, with the program aligned on them. That work is described in [the method](/guide/method), [Large programs, skeleton first](/guide/skeleton) and [The law](/guide/law); the case study is [Orbite](/orbite).

## 7. Conclusion

The separation between application code and its deployment is an accident of tooling. Putting distribution requirements in the source lets a compiler catch the contradictions among them, bound the memory they imply, and refuse what the runtime cannot do. It does not make a cluster correct. A proof of that needs a verified protocol, which Soma does not have, and this paper no longer suggests otherwise.

*Keywords: distributed systems, programming languages, model checking, compile-time checking of deployment declarations, eventual consistency.*
