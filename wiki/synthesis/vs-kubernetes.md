---
name: vs-kubernetes
description: Comparison with infrastructure-as-YAML (K8s, Helm, Terraform).
type: synthesis
since: V1.0
related: [scale, architecture, manifesto, vs-langchain, vs-erlang-pony]
---

# Soma vs Kubernetes

Kubernetes (with Helm, Terraform, GitHub Actions, etc.) is the
dominant cloud-deployment toolchain. Soma takes a different bet:
**deployment is a type-level concern of the code, not a separate
configuration layer**.

## What's the same

- **Replicas.** K8s `replicas: 50`; Soma `scale { replicas: 50 }`.
- **Sharding / sticky sessions.** K8s StatefulSets with consistent
  hashing; Soma `scale { shard: trades }`.
- **Resource budgets.** K8s `requests.memory: "256Mi"`; Soma
  `scale { memory: "256Mi" }`.
- **Health checks.** K8s liveness/readiness probes; Soma
  state-machine + [[ctl-model-checking]] (the "is it healthy"
  question becomes "is it in a terminal failure state").
- **Service discovery.** K8s Service; Soma `[peers]` in
  `soma.toml`.

## What's different

### 1. The deployment unit IS the code

K8s: `myapp.go` + `Dockerfile` + `deployment.yaml` + `service.yaml`
+ `configmap.yaml`. Five artifacts that have to stay coordinated.

Soma: one `mycell.cell` file. The `scale` section IS the deployment.

```soma
cell Service {
    scale { replicas: 50, memory: "8Gi", shard: data, consistency: strong }
    memory { data: Map [persistent, consistent] }
    on get(k: String) { data.get(k) ?? "not found" }
}
```

To deploy: `soma serve mycell.cell -p 8080 --join coord:9000`.

### 2. Cross-layer consistency is compile-checked

K8s: `deployment.yaml` says 3 replicas, `service.yaml` says
clusterIP, `configmap` says consistent reads — but if the app code
caches locally, you don't notice until production.

Soma: the compiler rejects inconsistencies:

```soma
memory { data: Map [ephemeral] }                  // local cache
scale  { shard: data, consistency: strong }       // strongly consistent
// ERROR: shard 'data' uses [ephemeral] but scale declares consistency: strong
```

This is the type-system payoff: distribution claims and storage
claims must agree at compile time.

### 3. Memory budgets are proven, not guesses

K8s: `memory: "256Mi"` is a hint to the scheduler. If the process
OOMs, K8s restarts it. No compile-time proof.

Soma: [[budget-proof]] proves peak ≤ declared at compile time. If
the bound exceeds the declaration, the compile fails:

```
$ soma check service.cell
error: budget exceeded in cell 'Service': proven peak 412 MiB > declared 256 MiB
```

OOM in production is a strictly smaller class of bug.

### 4. State machines are checked

K8s has no state machines. The application's state is whatever the
code does. Application bugs that leave state inconsistent show up at
runtime.

Soma's state machine is verified by [[ctl-model-checking]]. Bugs
like "every order should eventually reach `settled`" can fail at
*compile* time with a counter-example:

```
✗ eventually(settled) — counter-example:
  queued → running → failed → queued → ... (cycle)
```

### 5. Agent-native vs framework-bolted

K8s + LangChain on top is the common stack for LLM systems. There's no
verification across the boundary.

Soma's [[think-isolation]] proves state-machine safety **regardless
of what the LLM returns**. The "agent" isn't bolted on; it's the
language.

## What K8s has that Soma doesn't

Be honest:

- **Mature ecosystem.** Operators, CRDs, Istio, Prometheus, every
  cloud provider integration. Soma has the runtime + `--join`.
- **Multi-language support.** K8s pods can run anything; Soma pods
  run Soma cells.
- **Mature operator pattern.** Custom controllers, CRDs, finalizers.
- **Battle-tested at extreme scale.** K8s runs Google.
- **Production observability.** Built-in metrics, dashboards,
  logging integrations. Soma has `trace()` and a dashboard.
- **Multi-cluster federation.** K8s has tools for this; Soma has
  nothing.
- **Hot reload / rolling updates.** K8s does this natively; Soma's
  cluster mode is V1.0-shaped.

## Where Soma wins

- **No drift between code and config.** They're the same artifact.
- **Compile-time CAP analysis.** "Strong consistency on ephemeral
  storage" is rejected at compile time.
- **Budget proofs.** OOM-in-production becomes "compile error before
  deploy."
- **State machine verification.** "Order stuck in `pending`" becomes
  a CTL counter-example.
- **Agent integration.** No `LangChain pod` workaround.

## What "production K8s + Soma" looks like

For now, the realistic deployment:

- Soma cells run in containers like any other workload.
- Container goes into a K8s Pod.
- K8s handles the host-level concerns (node failures, network
  policies, secrets).
- Inside the Pod, Soma's `--join` clusters with other Pods.

The K8s layer becomes thin glue. The Soma source carries the
business / state / distribution intent. The K8s YAML carries
host-level concerns (node selectors, secrets injection, image
registries).

## Migration sketch

From K8s + Python service:

1. Pick one service. Identify its API endpoints → become [[face]]
   signals.
2. Identify its state → becomes [[memory]] slots with properties.
3. Identify its lifecycle → becomes a [[state-machine]].
4. Move `Deployment.replicas` → `scale.replicas`. Move
   `requests.memory` → `scale.memory`. Move `service.consistency` →
   `scale.consistency`.
5. Run `soma check` and `soma verify`. Resolve any issues that
   surfaced.
6. Deploy as a single artifact.

Steps 2–3 are the hard ones — making implicit state explicit. The
verification wins start there.

## Honest cut

K8s is **the** way to run general-purpose workloads in 2026. For
existing systems, "migrate to Soma" is a several-quarter project with
unclear ROI.

For **new systems with state, LLM components, and verification
requirements**, Soma's "code IS deployment + verification" thesis is
worth taking seriously even at lower maturity. The trade is: less
ecosystem, more correctness.

## Related

- [[scale]] — the distribution section.
- [[memory]] — properties that interact with sharding.
- [[architecture]] — the fractal cell model.
- [[verification-overview]] — what `soma verify` actually proves.
