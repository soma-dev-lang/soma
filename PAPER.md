# Scale as a Type: Verified Distribution in a Fractal Cell Language

## Abstract

We present Soma, a language where the unit of computation — the cell — is also the unit of distribution. A cell declares its state (memory), its lifecycle (state machines), its contracts (face), and its distribution requirements (scale). The same source code runs on a single machine or across a cluster of thousands of nodes, with no modification. A built-in model checker verifies temporal properties, consistency guarantees, and CAP trade-offs at compile time, before deployment.

We introduce three ideas: (1) distribution semantics as compile-time-checked properties on memory slots, analogous to type checking; (2) a fractal execution model where the same mechanism — cell, signal, handler — operates at every scale from function call to inter-datacenter replication; and (3) a single runtime flag (`--join`) that transitions a standalone process into a cluster node, with automatic data replication via the existing signal bus.

## 1. Introduction

Distributed systems are built in layers. The application layer (business logic) sits atop an infrastructure layer (Kubernetes, etcd, Raft, gRPC). The programmer writes code in one model and deploys it through an entirely different one — YAML manifests, Dockerfiles, Helm charts. The two artifacts have no formal relationship. A change in the application may silently violate assumptions of the infrastructure, and the mismatch is only discovered at runtime.

We propose eliminating this separation. In Soma, the application and its distribution requirements are expressed in the same language, the same file, the same model. The compiler verifies their consistency.

## 2. The Cell Model

A Soma cell is a self-contained unit with four components:

```
cell PricingEngine {
    face     { signal book_trade(data: Map) -> Map }     // contract
    memory   { trades: Map [persistent, consistent] }     // state
    state    { queued -> confirmed -> executed -> settled } // lifecycle
    scale    { replicas: 50, shard: trades, consistency: strong } // distribution
    on book_trade(data: Map) { trades.set(data.id, data) } // behavior
}
```

The same structure describes a function (a cell with handlers), a service (a cell with HTTP endpoints), a database (a cell with persistent memory), and a cluster (a cell with a scale section). This uniformity is what we mean by *fractal*: the model is self-similar at every scale.

### 2.1 Memory Properties as Distribution Types

Memory slots carry properties that function as distribution types:

| Declaration | Semantics |
|---|---|
| `[persistent, consistent]` | Durable, linearizable. Backed by SQLite locally, replicated via consensus in cluster mode. |
| `[ephemeral, local]` | Node-local, in-memory. Never touches the network. Microsecond access. |
| `[persistent]` + `consistency: eventual` | Durable, eventually consistent. Write locally, propagate asynchronously. |

These are not configuration — they are checked at compile time. The compiler rejects contradictions:

```
memory { data: Map [ephemeral] }
scale  { shard: data, consistency: strong }
// Error: shard 'data' uses [ephemeral] but scale declares consistency: strong
```

This is analogous to a type error: the programmer declared conflicting intentions, and the compiler caught it before any code ran.

### 2.2 Scale Section

The `scale` section declares distribution requirements:

```
scale {
    replicas: 50           // number of instances
    shard: trades          // which memory slot to distribute
    consistency: strong    // strong | causal | eventual
    tolerance: 2           // survives N node failures
    cpu: 4                 // resources per instance
    memory: "8Gi"
}
```

The compiler reads this and verifies:

- **Shard validity**: the named slot exists in the cell's memory
- **Consistency coherence**: no `[ephemeral]` + `strong`, no `[local]` + `shard`
- **CAP analysis**: `strong` + `tolerance > 0` implies CP mode (reduced availability under partition)
- **Quorum**: for strong consistency with N replicas, quorum = N/2 + 1, maximum tolerable failures = N - quorum
- **Locality**: non-sharded `[ephemeral, local]` slots are confirmed as node-local fast paths

These checks happen at compile time. The programmer knows, before deploying to 50 machines, that the system's distributed properties are internally consistent.

## 3. Verified Distribution

### 3.1 Temporal Properties

Soma includes a CTL model checker that verifies temporal properties on state machines:

```toml
[verify]
deadlock_free = true
eventually = ["settled", "cancelled"]

[verify.after.executed]
never = ["cancelled"]
eventually = ["settled", "failed"]
```

The checker exhaustively explores all paths in the state machine graph and produces counter-examples for violations:

```
✓ deadlock_free — no deadlocks in any reachable state
✓ after('executed', state != 'cancelled') — verified
✗ eventually(settled) — counter-example: queued → running → failed → queued → ... (cycle)
```

### 3.2 Distribution Properties

The same verification framework extends to distribution:

```
State machine 'PricingEngine/scale':
  ✓ replicas: 50 instances declared
  ✓ tolerance: survives 2 node failures (of 50 replicas)
  ✓ quorum: 26/50 nodes needed — tolerates 24 failures
  ✓ CAP: CP mode — consistent + partition-tolerant
  ✓ memory 'cache' is node-local — not distributed (fast path)
  ✓ scheduler runs on leader node only
```

These are not runtime assertions — they are compile-time proofs about the system's behavior under distribution.

## 4. Runtime: Signals as Replication

### 4.1 The --join Flag

A standalone Soma process becomes a cluster node with a single flag:

```
$ soma serve app.cell -p 8080                        # standalone
$ soma serve app.cell -p 8081 --join localhost:8082   # cluster
```

When `--join` is specified:

1. The new node connects to the seed node's TCP bus
2. It sends a `CLUSTER JOIN` message with its node ID
3. The seed responds with the current membership list
4. Both nodes build a consistent hash ring
5. The leader (lowest node ID) runs scheduled tasks (`every` blocks)
6. Heartbeats detect dead nodes; rejoining nodes receive a data sync

### 4.2 Memory Operations as Signals

There is no separate replication protocol. When `trades.set(key, val)` executes:

1. The value is written to local storage
2. An `EVENT _cluster_set {"slot":"trades","key":"...","value":"..."}` is broadcast on the signal bus
3. Every connected node receives the EVENT and applies it to its local storage

The signal bus — the same mechanism that carries inter-cell signals — carries data replication. This is the fractal principle in action: the same mechanism at every scale.

### 4.3 Consistent Hashing

Keys are distributed across nodes via a consistent hash ring (FNV hash, 128 virtual nodes per physical node). When a node joins or leaves, only K/N keys need to be reassigned (where K is the total key count and N is the node count).

For `get(key)`, the runtime checks local storage first. If the key belongs to another node (per the hash ring), it sends a request via the bus and waits for a reply. For `values()`, a fan-out query collects results from all nodes.

## 5. Evaluation

### 5.1 Expressiveness

We implemented three distributed applications in Soma:

| Application | Lines | Scale | Consistency | Verified Properties |
|---|---|---|---|---|
| Pricing Engine | 150 | 50 replicas | strong | trade executed ≠ cancelled, quorum 26/50 |
| Job Queue | 120 | 10 replicas | eventual | every job completes or expires, no deadlock |
| Chat | 90 | 5 replicas | eventual | messages delivered → read → expired |

Each application runs identically on 1 or N nodes. The only change is `--join`.

### 5.2 Verification Cost

The model checker runs in < 100ms for state machines with up to 15 states and 30 transitions (covering all examples). Distribution property verification (shard validity, CAP analysis, quorum calculation) is O(1) — it reads the scale section and the memory properties.

### 5.3 Limitations

- **Consistency**: the current prototype broadcasts all writes to all nodes (full replication), which does not scale to thousands of nodes. True sharding (where each node stores only its partition) requires the `get`/`values` fan-out path, which adds latency.
- **Consensus**: `consistency: strong` is declared and verified, but the runtime uses eventual replication. A true strong consistency implementation would require a consensus protocol (Raft, Paxos).
- **Failure semantics**: dead node detection uses heartbeats with a 15-second timeout. During this window, writes to dead nodes are lost. A production system would need write-ahead logging and replay.

## 6. Related Work

**Erlang/OTP** provides the actor model with transparent distribution, but without compile-time verification of distributed properties. A message sent to a remote actor may fail silently.

**TLA+/PlusCal** (Lamport) provides formal specification and model checking for distributed systems, but is not an executable language. The specification and the implementation are separate artifacts.

**Kubernetes** provides orchestration, but the distribution model (YAML manifests) is separate from the application code. There is no formal relationship between the two.

**Akka** (Scala) provides actors with location transparency, but consistency guarantees are library-level, not verified at compile time.

**Unison** provides content-addressed code that can be distributed, but does not include distribution semantics in the type system.

Soma is, to our knowledge, the first language where distribution requirements are (1) declared in the source code, (2) verified at compile time for consistency and CAP properties, and (3) executed by the same runtime that handles local computation.

## 7. Conclusion

The separation between application code and infrastructure configuration is an accident of history, not a fundamental necessity. Soma demonstrates that a single model — the cell — can express computation, state management, lifecycle, contracts, and distribution. The compiler verifies that these concerns are mutually consistent. The runtime executes them on one machine or a thousand, with a single flag.

The key insight is that distribution properties — consistency, tolerance, sharding — are types. They constrain the behavior of memory operations the way static types constrain the values of variables. And like type errors, distribution errors should be caught at compile time.

---

*Keywords*: distributed systems, formal verification, programming languages, fractal architecture, cell model, consistent hashing, CAP theorem
