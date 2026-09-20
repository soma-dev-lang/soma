# Experimental cluster runtime

The replication protocol described here is available since Soma 2.8.2 and is
included in the current [Soma 2.8.9 release](https://github.com/soma-dev-lang/soma/releases/tag/v2.8.9).

## What is implemented

A `scale` block with `shard: records` and `consistency: eventual` selects a
mutable Map for full replication. Despite the syntax name, entries are not
physically partitioned: each connected replica eventually holds the selected
slots. All cells are supported; `Cell.slot` identities keep same-named slots
separate. Other slots stay local. A resource-only `scale { memory: "32Mi" }`
does not enable the cluster or open a bus port.

Updates retain their stored types, including large integers, nested values,
empty strings and null. A failing handler or `try` block publishes no rolled
back writes. Remote writes are serialized with local handlers and checked
against the declared slot type. Replication cannot target an unselected slot;
legacy `_cluster_*` events cannot directly mutate storage.

Each key carries a `(Lamport counter, originating node ID)` version. The
higher version wins, including on a deletion. Reordered and duplicate
updates do not overwrite a newer version. This is a deterministic conflict
rule, **not wall-clock last-writer order**. Concurrent read-modify-write
operations can lose an application's increments. There are no cross-node
transactions, multi-key snapshots, linearizable reads, quorum acknowledgements
or consensus. `get`, `has`, `keys`, `values` and `len` read the local replica.

Discovery builds direct links between members, so leaves can communicate
after a seed stops. Each outgoing link retries failed connections with a
1–30 second backoff. State is exchanged on connection and roughly every
three seconds thereafter, including deletion markers. Persistent slots
require SQLite: data and versions commit in the same transaction. Each
replica must have its own project/data directory. Ephemeral data and its
versions are lost when the process stops. Keep the whole `.soma_data`
directory in backups; deleting cluster metadata can resurrect stale values.

## Start two nodes

Save this as `app.cell` in **two distinct directories**, `node-a` and `node-b`:

```soma
cell Replicated {
    memory { records: Map<String, String> [persistent, consistent] }
    scale { replicas: 2 shard: records consistency: eventual tolerance: 1 }
    on put(key: String, value: String) { records.set(key, value) }
    on get(key: String) { return records.get(key) }
}
```

Run in separate terminals:

```sh
cd node-a
SOMA_NODE_ID=127.0.0.1:8082 soma serve app.cell -p 8080
```

```sh
cd node-b
SOMA_NODE_ID=127.0.0.1:8092 soma serve app.cell -p 8090 --join 127.0.0.1:8082
```

The bus uses HTTP port + 2. Choose a fixed HTTP port in 1–65533 and avoid
HTTP, dashboard and bus port overlap. Across machines, bind the appropriate
interface with `--host` and set `SOMA_NODE_ID` to an address reachable by
peers. Seeds may also come from `SOMA_SEEDS` or `[cluster] seeds` in
`soma.toml`. A successful protocol acknowledgement establishes membership;
an unreachable or incompatible seed is retried, never reported as joined.
The cluster bus has no authentication or TLS: use a trusted private network.
Do not connect it to untrusted clients or mix unrelated applications.

## Scheduling and signals

After membership settles, `every` checks the lowest live node ID at each
tick. Missing heartbeats expire after 15 seconds, checked every three
seconds. The surviving node can then take over. This is **advisory
leadership**: discovery delays and partitions can produce simultaneous
ticks. There is no fencing, consensus lease or exactly-once scheduler.

Committed `emit` events reach connected cluster peers once per direct link.
They are fire-and-forget and are not part of state resynchronization; use
application IDs, deduplication and an outbox for reliable workflows. Do not
also declare the same nodes in `[peers]`, which creates a second event path.

## Verification and upgrade

`strong` and `causal` are rejected: the runtime does not implement their
protocols. Replicated Lists, immutable slots and memory invariants are also
refused. `replicas` and `tolerance` are declarations, not provisioning or
fault-tolerance proofs. `soma verify --strict` fails on the explicit warning
that distribution is unproved; a successful non-strict run is not a cluster
proof.

Protocol v2 is incompatible with 2.8.1 and earlier cluster traffic. Stop
all nodes and upgrade them together, retaining each data directory. Existing
local entries acquire replication metadata at startup. Do not run older
binaries or use `soma run` to mutate a live replica's store.

This implementation uses a full mesh, periodic full-state exchange and
retained tombstones, with at most 256 discovered peer addresses per node.
An encoded update must fit the 16 MiB bus line limit; larger local writes
are refused and rolled back. It is intended for small experimental clusters. It has no demonstrated
large-cluster throughput or bound on tombstone growth.

## Regression coverage

`compiler/tests/cluster.rs` starts real server processes with distinct stores.
It covers initial sync, types, rollback, same-named slots, event duplication,
concurrent writes during a TCP partition and convergence after healing, seed
failure, stale replica restart, tombstones, reordered
updates, private slot boundaries and scheduler takeover. Unit tests check
hash-ring balance/minimal movement and monotonic heartbeat expiry. These are
regression tests, not a model-checked proof of arbitrary partitions.
