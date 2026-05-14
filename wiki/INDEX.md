# Soma Wiki — Index

Read [CLAUDE.md](CLAUDE.md) (the schema) first if you've never used this wiki.

## Concepts — the mental model

- [[cell]] — the unit of computation, distribution, and verification
- [[face]] — public contract: signals, promises, tools
- [[memory]] — state slots with distribution-type properties
- [[state-machine]] — explicit lifecycle with compile-time verification
- [[handler]] — `on signal()` is the only way to add behavior
- [[scale]] — how a cell distributes (replicas, shard, consistency)
- [[think]] — bounded LLM call as a first-class primitive
- [[interior]] — sub-cells as composition mechanism

## Features — syntactic constructs

- [[sum-types]] — V1.5 tagged unions with exhaustiveness checking
- [[pattern-matching]] — `match` expressions, destructuring, guards
- [[ensure]] — postconditions enforced at runtime, structured for verification
- [[signals]] — `emit`, `await`, the inter-cell bus
- [[lambdas]] — `s => expr` for pipe-style data processing
- [[pipes]] — `|>` for `data |> filter() |> map() |> top(10)`
- [[record-literal]] — `User { name: "X", age: 30 }`
- [[duration-literal]] — `5s`, `1min`, `500ms`
- [[try]] — `try { expr }` and the `?` propagation operator

## Verification — what the compiler proves

- [[verification-overview]] — the four classes of guarantee
- [[ctl-model-checking]] — temporal properties on state machines
- [[refinement]] — V1.3: handler bodies cannot lie to the spec
- [[budget-proof]] — V1.4: peak memory ≤ declared budget
- [[think-isolation]] — safety holds regardless of LLM output
- [[termination]] — every handler structurally terminates
- [[composition]] — inter-cell signal matching
- [[coq-scorecard]] — what's mechanically proven, what isn't

## Stdlib — builtin reference

- [[stdlib-strings]] — `len`, `split`, `replace`, `contains`, `trim`, ...
- [[stdlib-math]] — `abs`, `sqrt`, `pow`, `random`, `gcd`, ...
- [[stdlib-collections]] — `list`, `map`, `push`, `with`, `nth`, ...
- [[stdlib-pipes]] — `|> filter`, `map`, `find`, `top`, `group_by`, ...
- [[stdlib-http]] — `response`, `html`, `redirect`, `sse`
- [[stdlib-storage]] — `data.set/get/delete/keys/values/len`
- [[stdlib-agent]] — `think`, `delegate`, `remember`, `recall`, `approve`
- [[stdlib-time]] — `now`, `now_ms`, `today`, `format_date`
- [[stdlib-linalg]] — `matrix`, `svd_lowrank`, `regress_sgd`, ...
- [[stdlib-risk]] — `impact_sqrt`, `var_historical`, `clean_covariance`, ...

## Synthesis — cross-cutting

- [[workflow]] — `generate → fix → lint → check → verify → serve`
- [[architecture]] — fractal cell model from function to cluster
- [[manifesto]] — the spec is the program
- [[vs-langchain]] — comparison with agent frameworks
- [[vs-erlang-pony]] — comparison with actor-model languages
- [[vs-kubernetes]] — comparison with infrastructure-as-YAML
- [[verified-pretrade]] — case study: empirical models as preconditions
- [[whats-missing]] — honest gaps in the current language

## Quick paths

**New to Soma:** [[cell]] → [[handler]] → [[state-machine]] → [[verification-overview]] → [[workflow]]

**Coming from LangChain:** [[think]] → [[stdlib-agent]] → [[ensure]] → [[budget-proof]] → [[vs-langchain]]

**Coming from Erlang/Pony:** [[cell]] → [[signals]] → [[refinement]] → [[vs-erlang-pony]]

**Coming from K8s:** [[scale]] → [[memory]] → [[architecture]] → [[vs-kubernetes]]

**Writing quant code:** [[ensure]] → [[stdlib-linalg]] → [[stdlib-risk]] → [[verified-pretrade]]
