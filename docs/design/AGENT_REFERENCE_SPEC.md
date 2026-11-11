# Soma as the reference language for LLM agents — gap specification

Status: 2026-11-10. Baseline: soma 2.8.10 (`agent-ux` @ 59667d7). The first tranche is implemented in 3.0.0 — see the status table.

A "reference language for LLM agents" has to win four arguments at once:

1. **Writable** — a model that has never seen Soma produces a correct program
   in fewer rounds than in Python or TypeScript, and this is *measured*.
2. **Trustworthy** — what `soma verify` prints is evidence a third party
   accepts without trusting the author, the model, or the Soma binary.
3. **Operable** — the agents Soma runs work against any provider, survive
   restarts, wait for people, and can be audited after the fact.
4. **Learnable** — the surface is small and stable enough that it stays
   correct in model weights between releases.

Soma today is strong on the *shape* of all four (per-cell proofs, fix-carrying
diagnostics, `cell agent` + budgets + hordes, an embedded agent guide). The
gaps below are what separates that from "reference". Each requirement has an
ID, the observed gap, the specification, and an acceptance test. Where a gap
was reproduced on 2.8.10, the reproduction is given; the rest is marked
*(by inspection)*.

## Status in 3.0.0

| ID | Requirement | 3.0.0 |
|---|---|---|
| A1 | Static types for expressions | **done** — per-handler inference; `T?` enforced in edition 2027. Not typed: reassigned locals, lambda parameters, match bindings, most builtin results, test cells. `[native]` keeps its own typing. |
| A2 | Parser recovery, structured syntax errors | **done** — statement / section / cell resynchronisation; semantic checks continue only when every syntax error left the AST complete (not "per cleanly parsed cell" as written below: a call into a cell that failed to parse would be reported as undefined). |
| A3 | Stable diagnostic codes | **partial** — codes, `soma explain`, a test that every checker `kind` has one. Codes are per kind (118 error kinds, 18 warning classes, 10 syntax classes), not per message; no `edits` field (`soma fix` applies the syntax fix-its); `gotchas.json` is not keyed by code. |
| A4 | Remove the rules that need a warning | **done, with two deviations** — `const`, `(a, b) => …`, `++`, `std.f(…)`; statement-form `xs.push(x)` / `m.set(k, v)` / `m.delete(k)` on locals; in edition 2027: `a ?? b + c` needs parentheses, `+` on non-numeric lists is refused, a bare call of a handler's name is never the builtin, a dropped `push(xs, x)` is an error, `get_status` of an unknown id is `()`. Deviations: `??` precedence is not changed (the same text would mean two things across editions and across an import boundary) — the ambiguous form is refused instead; as an EXPRESSION `xs.push(x)` on a local still returns a new list and leaves `xs` alone. The acceptance target (a 300-line core guide) is not met: the guide is 719 lines. |
| A5 | `soma fmt` | **partial** — canonical layout (indentation, blank lines, whitespace), `--check`; spacing inside a line is preserved. The corpus is not yet stored formatted (40 of 322 files differ). |
| A6 | Grammar artifacts, language server | **partial** — `spec/soma.grammar` → `spec/soma.gbnf`, `spec/grammar.md`, `editors/tree-sitter-soma/grammar.js`; the GBNF recognises all 1,411 `.cell` files `soma` parses (a test). `soma lsp`: check diagnostics on open / save, formatting. Not done: the tree-sitter grammar is generated but was never compiled; the GBNF was not run under a decoder; the grammar is loose (any balanced block) in `rules` / `runtime` / `scale` / `cost` / `protocol`; no hover, completion, go-to-definition, diagnostics for unsaved text. |
| A7 | Agent convergence benchmark | **partial** — `bench/agents`: 322 tasks from the corpus, runner, report; reference solutions 322 / 322, 86 % proven. Acceptance tests are given, not hidden; no Python / TypeScript arm; no model results published. |
| B1 | One semantics, three backends | **partial** — `tools/diff_fuzz.py` (interpreter vs native): 108,000 comparisons, no differing answer; a small campaign in the test suite. Open: 388 cases native code refuses (an Int past 64 bits in a Float expression, an exact quotient past 2^53) — announced, but the spec calls every divergence a bug; the fuzzer covers numeric handlers only (no strings, buffers, hashmaps); the documented `to_string(a / b)` difference stands. The bytecode VM is unreachable from the CLI and still in the tree: remove or finish is a decision left open. |
| B2 | Proofs that cross the cell boundary | **done, with limits** — face contracts (`requires` / `ensures`, enforced on every call, proven at call sites and returns, read across cells); `[verify.before."Cell.state"]` properties checked on the product of the machines over every run of the handlers, with the explored state count and a refusal past 200,000 states; `assuming = "…"` for a rule that depends on an edge guard; treasury's headline property is a `✓`. Limits: only `before` (precedence) is a product property — `never`, `eventually`, `after` stay per machine; the product over-approximates (a `✗` may show a run the data would never take); no contracts on `[native]` handlers. |
| B3 | Transition sources in the proof | **done** — sources inferred from `require` / `if` / `match` on `get_status`, aliases and earlier transitions; a move no known source allows is a verify failure. Deviation: `--strict` does not require the three-argument form where the source cannot be inferred (it stays a note). |
| B4 | Stronger invariant reasoning | **partial** — linear integer arithmetic behind the intervals: path conditions from `require` / `if` / early exits, slot invariants as assumptions on reads with a default, `-> Int` handler results; proves value invariants and relational `ensures` clauses. Deviations: a built-in Fourier–Motzkin procedure rather than an SMT solver (no dependency, deterministic; a bound on the work degrades to runtime-checked); no `match` path conditions; two-slot invariants are still enforced, not proven; no Float reasoning. |
| B5 | Evidence as an artifact | **done** — `verify --attest` / `--check-attestation`. Proof witnesses (the P2 part) not started. |
| B6 | Untrusted values | **partial** — a flow check, not a `Tainted<T>` type: model / HTTP values may not reach a transition target, a URL, a file path or `set_budget` unchecked (warning; error in 2027). Not covered: slot writes, tool arguments, flows through another handler; `verify` does not list the cleared sinks. |
| C1 | Typed model output | **partial** — `think_json(prompt, map("as", "Type"))` (not the `think<Type>` syntax): shape in the prompt, answer enforced, result typed at check. The provider's own structured-output schema is not sent; no repair round. |
| C2 | Provider-neutral model layer | **already partly there** — the OpenAI-compatible and Anthropic Messages wires with tool calls existed before 3.0. Not done: Gemini, streaming, image / document inputs, prompt caching, separate prompt / reply usage, the adapter conformance suite. |
| C3 | Durable waits | **partial** — `await approve(msg)` / `await signal(name, key)` / `await sleep(duration)` in a `[task]` handler: the step commits, position and locals are stored, the call answers 202, and `resume` / `deliver` / `POST /_approvals/<id>` / `POST /_signals/<name>/<key>` / serve's sleep timer run the rest, across restarts. Deviations: a wait is a whole statement of the handler body or of an `if` branch (not in a loop, lambda, `match`, `try`); no `ensures` on a waiting handler; a stored wait does not survive an edit of its handler (`wait_stale`); verify reports the resting state and its other exits per wait, it does not add the wait as a state of the machine's temporal check. |
| C4 | Replay that reproduces | **partial** — model answers recorded and replayed (`--live` for the old behaviour). Not recorded: tool results, HTTP responses, clock, random; not under `serve`. |
| C5 | Tools from outside | **partial** — `tool … [mcp: "server"]` / `[openapi: "server/operationId"]` bound to a `[tools.*]` table of soma.toml; `soma tools sync` pins each contract in soma.lock, `soma check` holds the declaration to the pin offline (kind `tool_binding`), a drifted server is refused at run time (kind `tool_schema`), capabilities apply to every call. Deviations: the schema is imported by `soma tools sync`, not by `check` itself (check stays offline); MCP over Streamable HTTP only (no stdio servers); OpenAPI documents in JSON only; argument types are compared at the top level (String / Int / Float / Bool / List / Map), nested schemas are not; no MCP resources, prompts or sampling. |
| C6 | Evals | **partial** — `soma eval` (a command over a JSON-lines dataset, not a `cell eval` construct): exact / contains / your grader, threshold, regressions against the stored run. No per-case token cost or traces. |
| C7 | Observability | **partial** — `SOMA_OTEL_FILE`: OTLP/JSON spans per top-level invocation with model / tool / approval / transition events, content off by default. A file exporter only (no OTLP/HTTP); steps are events, not child spans; no per-slot redaction rules. |
| C8 | Concurrency beyond one global lock | **partial** — under `soma serve` a read-only invocation runs on a snapshot of the last commit, outside the handler lock: readers in parallel, never behind a writer, never seeing half of one; an invocation taken for a reader that writes keeps nothing and runs again under the lock. Deviation: not "a lock per written key set" — writers stay strictly one at a time (one SQLite file, one write transaction); no snapshot with an in-memory slot, a `[native]` handler, a cluster or a `[task]`. |
| C9 | Embedding and interop | **partial** — `soma embed` (handlers over stdin / stdout, typed values, errors by kind) and two clients without dependencies, `packages/python` and `packages/npm` (with TypeScript declarations), tested from the suite. Deviations: the packages are not published (`pip install soma` / `npm i soma` do not exist yet) and do not bundle the binary; an out-of-process protocol, not an in-process binding; `[native]` handlers run interpreted there. |
| D1 | Editions | **done** — `edition` key, 2026 default, 2027 written by `soma init`, `SOMA_EDITION`, `soma fix --edition 2027`. An imported file is checked under the importing program's edition. |
| D2 | Split agent guide | **partial** — the language (699 lines, was 992), agents, native and releases are separate files and `soma docs` topics. Not at the 300-line target: that needs the A4 removals to become the default edition. |
| D3 | Ecosystem floor | **partial** — eight packages in pure Soma with their tests and registry entries: `money`, `time` (zones by rule), `auth` (JWT HS256, signed payloads), `ratelimit`, `queue`, `text`, `vector`, `http_client` (the spec's `http-client`: a package name is an identifier). Not done: the Postgres and S3-like adapters; time zones from the tz database; the registry is not deployed; the "20 most common service tasks" target was not measured. |
| D4 | Governance | **partial** — `SECURITY.md`, `docs/SOUNDNESS_LOG.md`, `docs/rfcs/`. The versioned conformance suite is the existing test suite, not a separately runnable package. |

Priorities: **P0** blocks the claim, **P1** is needed for adoption, **P2**
widens the lead.

---

## A. Writable

### A1 (P0) — Static types for expressions

**Gap (reproduced).** `soma check` types literals only. These pass `check`
and fail at run time:

```soma
cell type Line { variants { Line { sku: String, qty: Int } } }
on total(lines: List<Line>) { for l in lines { t += l.quantity } }  // no such field
on bad() { let s = "abc"   let n = s * 2 }                           // String * Int
```

`total(5)` and `return "x"` against `-> Int` *are* caught — but only because
the offending value is a literal. A language whose pitch is "errors before
anything runs" cannot leave field typos to run time.

**Spec.**
- Local, bidirectional inference; no new annotations. Sources of types:
  literals, parameter types, face return types, slot declarations, `cell type`
  fields, and the builtin signatures already in the registry
  (`soma describe --builtins --json`).
- The inferred type of every `let` flows forward; reassignment to an
  incompatible type is an error (already the rule in `[native]`).
- Absence is a type: `T?`. `m.get(k)`, `m[k]` on a Map, `nth`, `recall` yield
  `T?`; arithmetic or field access on `T?` is a check error whose fix is
  `?? default`. This turns the `counts[k] += 1` gotcha into a diagnostic.
- `Any` remains the explicit escape; a value of type `Any` is checked at run
  time exactly as today. Unannotated lambdas are inferred from their use site.
- Record field access on a known record type is checked (field exists, type
  flows). Field access on `Map` stays dynamic.
- `try { e }` has type `Try<T>`; `.value` is `T?`. Closes ledger item D5
  (`.url` on a try result).

**Acceptance.** The two programs above are check errors naming the field /
operand. All 322 corpus programs and the six demo apps check with zero new
errors, or each new error is a real defect. No annotation is added to any
existing program.

### A2 (P0) — Parser recovery and structured syntax errors

**Gap (reproduced).** The parser stops at the first syntax error; a file with
`-> Int` on a handler, `=>` in a match, and `(a, b) =>` takes three rounds.
In `check --json` a syntax error is `{"kind": "load", "message": "… (app.cell:16:27)"}`
— no `file`/`line`/`col`/`fix` fields, unlike semantic errors.

**Spec.** Statement- and item-level recovery (resynchronise on newline at
brace depth, on `on`, `cell`, `}`); report every syntax error in one pass,
then run the semantic checks on the recovered AST where a cell parsed
cleanly. Syntax errors carry the same JSON fields as semantic ones.

**Acceptance.** A fixture with five independent syntax errors yields five
diagnostics in one `check`, each with `file`, `line`, `col`, `fix`.

### A3 (P0) — Stable diagnostic codes

**Gap (by inspection).** Diagnostics have a `kind` and prose, no stable
identifier. Agents and docs match on substrings.

**Spec.** Every diagnostic gets a code (`E0142`, `W0031`) that never changes
meaning; `soma explain E0142` prints cause, wrong→right pair, and a corpus
program that shows the right form. `gotchas.json` is keyed by code. JSON
diagnostics gain `code` and, where mechanical, `edits: [{range, replacement}]`
so the fix is applied without re-generation (`soma fix` consumes the same
field).

**Acceptance.** 100 % of diagnostics emitted over the regression suite carry
a code; a CI check refuses a new diagnostic without one.

### A4 (P1) — Remove the rules that need a warning

A rule the compiler has to warn about is a rule the language should not have.
Each of these is current behaviour (the first three reproduced on 2.8.10, the
rest as documented in `llms.txt`):

| Today | Spec |
|---|---|
| `[1,2] + [3,4]` → `[4, 6]`, `["a"] + ["b"]` → `["a","b"]` (meaning depends on contents) | `+` on lists is element-wise only and a type error otherwise; `++` (or `concat`) joins. |
| `m.a ?? 0 + 1` parses as `m.a ?? (0 + 1)` | `??` binds tighter than arithmetic, as in C#, Swift, Kotlin, JS. |
| lambdas take one parameter; `reduce` takes `p => p.acc + p.val` | `(a, b) => …` |
| `f(args)` resolves to handler or builtin by arity | a handler named like a builtin always wins inside its program; calling the builtin is `std.f(…)`. |
| `.set`/`.push` exist on slots, not on local maps/lists | one vocabulary: `m[k] = v` and `xs.push(x)` work on both; a discarded `push(xs, x)` result is an error. |
| `get_status(id)` answers the initial state for an unknown id | returns `()`; `has_state` becomes redundant. |
| no constants; proven bounds repeat the literal | `const LIMIT = 500` at cell level, usable in invariants, `require`, guards, and by the prover. |

These are breaking; they ship under an edition (D1).

**Acceptance.** The "Coming from Python/TypeScript" and "Things you would
otherwise guess" sections of `llms.txt` shrink by at least half; the core
agent guide (syntax + cell anatomy + tests, no release notes) fits in 300
lines.

### A5 (P1) — Canonical form: `soma fmt`

**Gap (by inspection).** No formatter. Without one, corpus programs, diffs
and model outputs have no canonical shape, and training data teaches several
layouts.

**Spec.** `soma fmt` (idempotent, comment-preserving, no options);
`soma fmt --check` for CI. The corpus and every doc snippet are stored
formatted.

### A6 (P1) — Grammar artifacts for editors and constrained decoding

**Spec.** One grammar source generates: `spec/grammar.md`, a tree-sitter
grammar, and a GBNF / JSON-schema-style grammar usable for constrained
decoding. A language server (`soma lsp`: diagnostics, hover with inferred
types from A1, go-to-definition, the `edits` of A3 as code actions).
The grammar test corpus is every `.cell` file in the repository.

### A7 (P0) — A public benchmark of agent convergence

**Gap.** The agent-UX ledger is the right method (fresh agents, friction
logs) but it is internal and qualitative. "Reference language" is a
comparative claim and needs a number.

**Spec.** `bench/agents/`: ≥ 100 tasks with hidden tests across the domains
of the corpus, each specified in prose, each with a Python and a TypeScript
reference harness. Metrics per model: first-pass `check` rate, rounds to
green, tokens to green, and — the Soma-specific one — *fraction of stated
safety properties that end up proven rather than tested*. Published per
release for at least three model families; a release that regresses the
headline metric is not a release.

**Acceptance.** Results page generated by `tools/build_site.py` from raw run
logs; runs reproducible from a single command.

---

## B. Trustworthy

### B1 (P0) — One semantics, three backends, enforced

**Gap (by inspection of the docs).** The interpreter is the reference; the
VM is "incomplete"; `[native]` documents divergences (an exact-quotient
division by zero answers `inf`/`NaN` natively where the interpreter raises;
a quotient's type depends on context). The 2.8.x changelog is largely a list
of backend divergences found one at a time.

**Spec.**
- A normative `docs/SEMANTICS.md` that the interpreter implements; every
  divergence is a bug, not a documented difference.
- A differential fuzzer in CI: generated programs in the native-eligible
  subset run on all backends; any observable difference fails the build.
- The bytecode VM either passes the same suite or is removed. Two
  half-equivalent backends cost more trust than one slow one.

**Acceptance.** Zero entries under "one difference" / "scope" in the
Performance section of `llms.txt`; fuzzer runs ≥ 10⁶ programs per release.

### B2 (P0) — Proofs that cross the cell boundary

**Gap (stated in the README).** Verification is per cell; composition is
linted. Every demo app with more than one cell (treasury: Ledger + Vendors +
agent) rests its headline property on an unproven seam.

**Spec.**
- Product model checking over the machines of cells linked by direct calls
  and in-process `emit`, with the same property vocabulary
  (`[verify.before."Pay.paid"] requires = ["Approval.signed_off"]`).
- Assume/guarantee contracts on `face`: a signal may declare `requires` /
  `ensures` over its parameters and result; callers are checked against
  `requires`, the handler against `ensures`, and the prover uses `ensures`
  at call sites (today a number read through a handler is "unbounded").
- State-explosion honesty: verify prints the explored state count and
  refuses, rather than truncates, past a published limit.

**Acceptance.** treasury's "no path to `paid` over the limit without
`signed_off`" is a `✓`, not a comment that says "pinned by tests".

### B3 (P1) — Transition sources in the proof

**Gap (reproduced).** verify notes: "`open`, `close` call transition(id,
target) without declaring the source — the proof does not see where the
handler starts".

**Spec.** Infer the source set per `transition()` from preceding
`require get_status(id) == …` / `match get_status(id)`; where it cannot be
inferred, `--strict` requires the three-argument form. Refinement then
proves edges, not just targets.

### B4 (P1) — Stronger invariant reasoning

**Gap (reproduced).** `acct.set(id, b - amount)` with `b` read from the slot
is "runtime-checked" until the author adds a `require` restating the
invariant. Interval reasoning only; relations between two slots are never
proven.

**Spec.** An SMT backend (linear integer arithmetic) behind the existing
interval domain: path conditions from `require`/`if`/`match`, slot
invariants as assumptions on reads, two-slot invariants as proof
obligations. Solver time-outs degrade to runtime-checked exactly as today —
never to "proven".

### B5 (P1) — Evidence as an artifact

**Gap (by inspection).** `verify` output is text for the author. An agent
"hands back evidence" by pasting it.

**Spec.** `soma verify --attest out.json`: source hash, toolchain version
and build hash, every property with its verdict class
(`proven` / `runtime-checked` / `assumed`), counter-examples, explored state
counts. Deterministic bytes for a given source + toolchain, so a reviewer
re-runs and compares hashes. Later (P2): exportable proof witnesses
(TARGET_SOMA_V1 §1) so the checker itself need not be trusted.

### B6 (P2) — Untrusted values as a type

**Gap (by inspection).** treasury defends against a prompt-injected model
by hand: the model's number is compared with the invoice before it reaches
the ledger. Nothing forces the next author to do the same.

**Spec.** Values returned by `think*()`, by tool results, and by `http_*`
carry a `Tainted<T>` type. A tainted value cannot be a `transition()`
target, a slot write, a URL, or a tool argument until it passes a
`require`, a `match` against literals, or a declared validator handler.
`soma verify` lists every sink a tainted value reaches and the check that
cleared it. This is the property "the model proposes, the language
disposes" stated as a type rule.

---

## C. Operable

### C1 (P0) — Typed model output

**Gap (by inspection of `builtins/llm.rs`).** `think_json` requests
`response_format: json_object` and returns a Map; the shape is whatever the
model chose.

**Spec.** `think<Decision>(prompt, opts)`: the JSON schema is derived from
the `cell type`, sent as the provider's structured-output schema where
supported, and the reply is validated into the variant (kind `json` on
mismatch, with one bounded repair round as an option). With A1 the result
is statically a `Decision` and the following `match` is exhaustive.

### C2 (P0) — Provider-neutral model layer

**Spec.** A provider interface with first-class adapters for the major
wire formats (OpenAI-chat-compatible, Anthropic Messages, Gemini) covering:
tool calls, structured output, streaming, image/document inputs, prompt
caching, and *reported* usage for budgets (prompt and reply tokens
separately, so `cost { tokens }` can bound both). A conformance suite runs
each adapter against a recorded fixture server. `[agent]` selects provider
and model per cell.

**Acceptance.** The same `cell agent` with tools passes its tests against
all adapters without source changes.

### C3 (P1) — Durable waits

**Gap (documented).** `approve()` under `serve` raises
`approval_required`; the documented pattern is to hand-build a pending state
and a second endpoint. `[task]` handlers commit per step but cannot wait for
anything except a model.

**Spec.** In a `[task]` handler, `await approve(msg)`, `await signal(name,
id)`, and `await sleep(duration)` end the current step, persist the
continuation, and resume — after a restart too — when the answer arrives
(`POST /_approvals/<id>` or a program-defined route). The waiting state is
visible to verify as an implicit machine state, so liveness properties see
it.

### C4 (P1) — Replay that reproduces

**Gap (documented).** `soma replay` calls the model again; replies are not
recorded. That is a regression harness, not an audit trail.

**Spec.** `--record` stores every nondeterministic input (model replies,
tool results, HTTP responses, clock, random) keyed by call site and
sequence; `soma replay` consumes them and must reproduce every write and
transition byte-for-byte; `replay --live` keeps today's behaviour.
Recording is available under `serve`.

### C5 (P1) — Tools from outside

**Spec.** A `tool` may be bound to an external tool server (MCP client) or
an OpenAPI operation instead of a handler; its schema is imported at check
time and pinned in `soma.lock`; capabilities scope it like any other tool.
(This is Soma *consuming* tools. It does not bring back a Soma MCP server.)

### C6 (P1) — Evals as a language construct

**Spec.** `cell eval Name { dataset "cases.jsonl"  run h(case.input)
grade exact | contains | judge(Cell.handler) | custom  threshold 0.9 }`,
run by `soma eval`, with per-case traces, token cost, and a comparison
against the last stored run. Prompts change behaviour the way code does;
they need a regression gate of the same standing as `cell test`.

### C7 (P1) — Observability

**Spec.** OpenTelemetry export (GenAI semantic conventions) of every
handler invocation, `think`, tool call, transition, refused write and
budget event; `trace()` stays as the in-process view. Redaction rules
declared per slot/field.

### C8 (P2) — Concurrency beyond one global lock

**Gap (documented).** Under `serve`, handlers run one at a time.

**Spec.** Lock per machine instance / per written key set, inferred from
the handler's static read/write footprint; handlers with disjoint
footprints run in parallel; the atomicity guarantee is unchanged.

### C9 (P2) — Embedding and interop

**Spec.** `pip install soma` / `npm i soma`: load a `.cell`, call handlers,
receive typed values and errors by kind — so Soma can be the verified core
of an existing Python or TypeScript agent stack rather than a replacement
for it. The reverse direction stays the provider sidecar.

---

## D. Learnable

### D1 (P0) — Editions and a compatibility promise

**Gap (by inspection of the changelog).** Ten patch releases in the 2.8
line changed observable semantics (dispatch rule, `==`, division typing,
storage failure behaviour). Each was a fix, but a model trained on last
quarter's Soma writes last quarter's Soma.

**Spec.** `edition = "2027"` in `soma.toml` (default: the edition of the
toolchain that ran `soma init`). Breaking changes — all of A4 — land only in
a new edition; `soma fix --edition 2027` migrates mechanically; old editions
keep compiling for a stated period. Within an edition: no program that
passes `check` + `test` changes behaviour except for a soundness fix, and
each such fix is listed with its diagnostic code.

### D2 (P1) — Split the agent guide

**Gap (reproduced).** `site/llms.txt` is 951 lines; the first hundred are
release notes, and much of the rest is edge-case prose.

**Spec.** `llms.txt` = the language (≤ 300 lines, no release history, every
snippet checked by `build_site.py`). Release notes, operations, agents and
native live in separate fetchable files. Each paragraph that describes an
exception is tagged with the A4 item or diagnostic code that will remove it.

### D3 (P1) — Ecosystem floor

**Gap (by inspection).** One package (`matrix`) in the registry.

**Spec.** A standard library shipped as packages with proofs: `http-client`
(retries, pagination), `auth` (JWT/HMAC), `time` (zones — today there are
none), `money`, `queue`, `ratelimit`, `text` (tokenise, diff), `vector`
(embeddings + similarity), plus adapters for at least Postgres and S3-like
storage. Target: an agent never has to leave Soma for the 20 most common
service tasks in A7.

### D4 (P2) — Governance

**Spec.** The normative spec, the conformance suite (B1) and the benchmark
(A7) are versioned together and can be run against any implementation. A
public RFC process for edition changes. A security policy with a disclosed
soundness-bug log (every "proved something false" incident, like the
`after … never` encoding found in cycle 1).

---

## Order of work

1. **Foundation** — A1, A2, A3, B1, D1. Nothing else is credible on top of
   literal-only type checking and backends that diverge.
2. **The claim** — A7, B2, B5, C1, C2. This is the minimum for saying
   "reference" in public: a number, a cross-cell proof, an evidence file,
   typed model output on any provider.
3. **Edition 2027** — A4, A5, A6, D2, B3, B4.
4. **Production** — C3, C4, C5, C6, C7, D3.
5. **Lead** — B6, C8, C9, D4.

## Non-goals

- Becoming a general-purpose language. Soma wins where there is a lifecycle
  or a limit; numeric speed stays `[native]`'s job.
- An "intent" natural-language layer above cells (VISION §1). It moves the
  unverified step from the model into the compiler; the gain is in A1–A3.
- A Soma MCP server (removed March 2026; decision stands).
