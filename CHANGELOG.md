# Changelog

## Unreleased

- Storage: a write the database refuses (a read-only `.soma_data`, a full
  disk) raises kind `storage` and rolls the handler back — `set` / `push`
  were silent no-ops answered 200; `rows[i]` on a persistent List is an
  indexed read again (20 000 reads: 0.09 s, was 0.72 s; 100 000 took 17 s).
- Packages: `soma install` reinstalls a `path` dependency whose source
  changed (every build and test kept running against the first copy).
- `soma test <dir>` / `soma check <dir>` run every .cell under a directory.
- `soma serve`: SIGINT stops it (a shell that starts it in the background
  leaves SIGINT ignored); `-w` checks the new file before stopping the
  running one, and says when the server died; `-p 0` prints the port the OS
  gave; a damaged database is refused before the banner.
- CLI: a Map argument carrying `_type` / `_variant` / `_values` at any depth
  is refused, as over HTTP.
- The prover reaches further: `x % n` / `mod(x, n)`, `x * x`, `len("abc")`,
  `idiv`, `sqrt_int`, `band(x, mask)`, and the documented monotone counter
  `invariant value >= (seq.get(key) ?? 0)` written as `+ n` or `max(…)`.

- Soundness: `vote()` is in the call graph of every proof — a size
  invariant past a vote whose target wrote the slot was "proven" (and
  raised), recursion through vote was "proven" to terminate, its latency
  counted one voter instead of k; `horde()` targets and callbacks are in
  the termination graph (a horde restarted from on_done ran 20 821 times
  under "✓ terminates").
- The wait for a rate limit (`SOMA_LLM_RPM`, `[agent] rpm`) and a mock's
  latency are inside the think's `timeout` (a "proven" 2 s took 60 s).
- Bus: an event emitted while a `[peers]` peer's link is down is logged NOT
  delivered to it, even while other links are up (50 alerts lost, 3
  logged); docs: `emit` reaches every link, receivers filter by `[bus]
  accept`.
- `"{Other.ask(1)}"` is fine again; `range(a, b, step)` has a known length
  for cost bounds; docs: what `usd` counts.

- Budgets: `set_budget(0)` means no more model calls (it meant "no
  limit": a computed `quota - spent` reaching 0 lifted the cap); a negative
  budget is refused; `vote()`'s concurrent voters reserve against what
  `set_budget` left and their tokens and trace count for the caller (25
  voters spent 1 550 tokens under `set_budget(50)`, uncounted); a horde
  without `budget_tokens` counts every think of it (callbacks, votes, nested
  hordes) in `horde_status.tokens`; check warns on a computed `budget_tokens`.
- `http_get` / `http_post` never turn a remote JSON with `_type` /
  `_variant` into one of your records (a reflected `Admin` matched the Admin
  arm): such a body stays text.
- `"{len}"`, `"{helper}"`, `"{Cell}"` are check errors (a function or cell
  is not a value); `"{true}"` / `"{false}"` interpolate.
- `[task]` lints: a read in an `if` around the think, through a helper, or a
  write through a helper is stale; a value re-read after the think is not;
  `transition()` / `emit` before a think in a `try` count as writes; a
  literal `delegate` to a thinking handler is a think.
- Termination: `map("k", v)` in a lambda is not a function call, and a
  parameter typed as data is not a function value (`verify --strict` failed
  on CSV-row building).
- An Int given for a `Float` field or parameter becomes a Float.

- Soundness: a `[task]` tick (`every … [task]`) and `vote()` end a step —
  the prover keeps no fact across them (a "proven" `members.size <= 3` was
  broken at run time); the stale-read, try-write and no-think lints follow
  helpers, other cells' agents, emit listeners and `"{think(…)}"`, and
  apply to `[task]` ticks.
- Hordes: a horde whose task starts a horde of itself is flagged by verify
  and stopped (hordes started from tasks nest at most 4 deep; rounds started
  by on_done do not nest; at most 1 000 000 queued tasks per process); a
  server that lost a horde's lease stops recording its tasks (results after
  on_done); a horde's creator no longer resumes it a second time (double
  concurrency); `on_error` counts in the cost bound; `seed` covers apply and
  on_done; `instance` memory is per owner cell; status says done inside
  on_done; check: `seed` / `instance` / on_done parameter types.
- Records: `r.field += 1`, `r["field"]`, `xs[0].field = v`, `with(record, …)`;
  a field write must match its declared type.
- Strings: `"{\"a\": {\"b\": 1}}"` kept its last `}` — a `}` closing a
  literal `{` is never half of a `}}` escape.
- A handler another handler called can only lower the caller's `set_budget`.
- Docs: `SOMA_LLM_MOCK=rules:<file>` format; `keys(record)`.

## 2.7.0 — 2026-09-19

- Hordes (attack pass): nested hordes and hordes started from callbacks run
  under their parent's budget (a nested horde spent 6 000 tokens past a
  "proven" 100); check refuses a computed target or options (an HTTP client
  could pick a private handler or drop the budget) and literal options out of
  range, and warns on public callbacks; two servers on one `.soma_data` run
  each horde once (a lease, taken over when its server stops); hordes are
  persisted under serve / run even without a `[persistent]` slot; the queue
  tables cannot be reached from a slot named `_hordes`; only the owner cell
  reads or cancels a horde; the rate limiter is an exact 60 s sliding window.
- Hordes (quality pass, phase 5): a crash while a horde was cancelling
  resumes it as cancelled with counts that add up; a restart does not resume
  a horde whose handlers the new code renamed (it says why, state `paused`);
  under `soma test` a horde runs after the caller commits, like serve (a
  callback can cancel it); `on_error` receives `{error, kind, detail}`;
  tasks the budget stopped count as cancelled; `soma verify` prints each
  horde bound once and `--strict` fails on an unbounded one; each task
  starts with a fresh model context. Dashboard: live hordes at `/__soma/`.
  `SOMA_LLM_MOCK=rules:mocks.json` answers by prompt pattern. Corpus:
  `agents/horde_audit.cell`, `agents/horde_rounds.cell`.
- Hordes (phase 4): rounds for simulations — `snapshot` (every agent sees
  the same world), `apply` (results applied at the end in input order, once),
  `seed` (reproducible `random()` per task), `instance` (per-agent
  remember/recall and conversation across rounds); `vote(handler, input, k)`.
  10 000 agents × 20 rounds: 46 s, identical on every run.
- Hordes (phase 3): `budget_tokens` is a hard ceiling — each think() of a
  horde reserves an upper bound (request bytes + max_tokens) before calling
  the model, waits outside the lock for calls in flight when it does not fit
  yet, and is refused (kind `budget`, state `exhausted`) when it never will.
  `soma verify` prints each horde's cost bound; in a cell with `cost { }` a
  horde over inputs of unknown size needs a literal `budget_tokens`.
- Hordes (phase 2): `horde(Reviewer.review, docs, map("concurrency", 200,
  "on_result", "_store", "on_done", "_done"))` runs a `[task]` handler once
  per input with a bounded pool and returns an id at once; `horde_status`,
  `horde_results`, `horde_cancel`. The queue is persisted with the data and
  resumes after a restart; each result is recorded (and `on_result` called)
  with the task's last step, so once even after `kill -9`. Retries
  (`max_attempts`), `on_error`, `on_done`. Provider limits `[agent] rpm` /
  `tpm` (SOMA_LLM_RPM / SOMA_LLM_TPM) shared by every think(). 10 000 tasks
  with a 2 s mocked model at concurrency 500: 41 s. Check validates the
  target, callbacks and options.
- `[task]` handlers (hordes, phase 1): `on h(…) [task]` runs as steps —
  each `think()` commits the current step and waits for the model outside
  the handler lock, so concurrent requests overlap their model calls (200 ×
  2 s mocked calls in ~9 s). A failure rolls back the current step only; the
  prover carries no fact across a `think()`. Ticks take it too:
  `every 1min [task] { … }`, `after 5s [task] { … }`.
  `SOMA_LLM_MOCK_LATENCY_MS` gives the mock a latency.
- Check warns when a plain handler or tick (`on request` routing, a
  listener) calls a `[task]` handler — it would hold the lock; when a
  `[task]` reads a slot before a `think()` and writes it after; when a `try`
  writes and thinks; when a `[task]` has no `think()`. `[task, native]` and
  unknown handler annotations (`[tsak]`) are errors.
- Records: `r.field = v` on a record, `is_a(r, "Line")`, `keys(r)` /
  `values(r)`; a record prints `Line { sku: a, qty: 2 }`.
- Cost: a loop over a collection of unknown size counts once (a lower
  bound, the bound stays advisory) instead of an invented ×100 that could
  report a false "budget exceeded"; verify counts think() call sites.
- "undefined function" points at the call, not the handler header.

- Scheduler: a second `soma serve` takes over the every/after blocks when
  the owner stops; the lock is per program; each tick's token budget starts
  fresh.
- Records: a JSON object / Map given for a one-variant `cell type` becomes
  that record (errors name the field); variant fields read as `v.field`;
  deleting from an `[immutable]` slot is a check warning.
- Your handler named `subscribe`, `link`, `ws_connect` or `ws_send` wins
  over the network builtin at its arity; `cell test` helpers are held to
  the declaring cell's invariants and `[immutable]`.
- `soma run` checkpoints the WAL on exit; a damaged BigInt row is reported
  instead of reading as 0; `--fresh` resets data only after the check
  passes; handlers reached from `request` through a model tool are not
  endpoints; `ensure` after an early `return` is a check warning; a raising
  `forall` names its value; notes are not counted as warnings.
- Soundness: a `require` on a slot read no longer proves writes made after
  the slot was rewritten (here, through a helper or `delegate`); handlers
  reached from `request` through an `emit` at any depth are not endpoints.
- `[immutable]` slots are enforced: append-only Lists, add-only Maps.
- CSV "NaN" / "inf" stay text; `soma test` never touches `.soma_data`;
  `delegate` keeps the callee's error kind; `soma run` refuses a damaged
  database; `--fresh --record` starts a new log.
- Soundness: a negative List index is checked at its real index; a List
  delete re-checks shifted elements against `key` invariants (verify
  reports it runtime-checked); a self-recursive writer counts for size
  proofs; untyped / Any slots get no integer narrowing; file writes and
  `subscribe` make a latency bound advisory, `subscribe` has connect and
  handshake timeouts.
- `()` is refused for a List parameter; a slot-less invariant over several
  slots is a check warning; `write_file` / `write_csv` create the directory;
  `to_csv` keeps every column.
- Site: external effects are not rolled back (/agents); replay re-runs LLM
  and HTTP calls live (README).
## 2.6.1 — 2026-09-19

- Site and README: the stated guarantees now match the documented ones —
  distribution checks prove the declarations' coherence (the prototype
  replicates eventually), rollback covers slot writes and transitions (not
  external effects), `set_budget` stops the next call, default liveness
  means an exit stays reachable (`eventually` for every run), the paper's
  quorum is ⌊N/2⌋ + 1; the /agents payment example requires `amount > 0`;
  a homepage demo shows proven / runtime-checked / not covered.
- `soma run --fresh` resets exactly this program's tables (a cell `A`
  reset another program's `A_b`); an unreadable soma.toml fails closed;
  `soma run` without a handler prefers `main` / `run` and never runs a
  `_private` one; `soma build -o x.cell` is refused and `soma deploy`
  keeps existing files; `verify --json` is JSON when check fails.
- `soma fix` handles non-ASCII lines; `soma replay` keeps numeric-looking
  Strings as Strings and fails on unreadable or empty logs; the dashboard
  is same-origin only.
- New `asin`, `acos`, `pi()`; `tan`, `atan`, `atan2`, `asin`, `acos`, `pi`
  in `[native]`.
- Bus: a connection must send its first line within 10 s; at most 256
  are open at once (half-open connections held a thread each).
- `require <Int builtin>` is a check error like `if`; `soma run --fresh`
  resets only this program's tables when other programs share the database.
- New `to_csv(rows)`; `round` never returns -0.0; clearer errors for
  negative `round` digits and multi-variable `forall`; recursion limits
  documented (512 interpreted, 20 000 `[native]`).
- Peer bus: two processes listing each other exchange each event once
  (links open with `HELLO`); closed connections free their thread and
  socket; a `[peers]` address that is this process is refused; the
  reconnect back-off holds for links that drop at once.
- The start-up audit no longer reports write-once rows; a prompt that
  alone overruns the remaining `set_budget` raises before it is sent.
- `body: String` is the exact bytes received; `to_json` escapes `</` and
  `<!--`; the injected htmx script is pinned with SRI; `hmac_sha256` with
  an empty key fails closed; `--record` logs are owner-only.
- `[peers]` links are supervised: a peer down at start-up, restarted, or
  dropped for reading too slowly is reconnected.
- Packages: sub-directories are installed and covered by the lock's
  sha256; a file the lock does not list, or a case-variant `use`, cannot
  bypass the check; an installed package missing from the lock is refused.
- Latency bound: http without timeout counts 30 s; a think without a
  literal timeout makes it advisory. The JSON cap weighs objects and lists;
  `self_call` catches IPv4-mapped IPv6; CSV cannot carry `_type` /
  `_variant` columns; invariant source is not sent to clients; duplicate
  Origin headers are refused; imported `every` / `after` is warned.
- `split(s, "")` splits into characters; `parse_int(s, base)`; numbers
  beyond the Float range are refused by `from_json`, HTTP bodies and bus
  events; NaN sorts last in descending `sort_by`; `ipow` of 0 / ±1 to huge
  exponents; `sum_by` of a non-list raises.
## 2.6.0 — 2026-09-19

- soma.lock records a sha256 of each package's files: a package modified
  after install is refused at import, and `soma install` restores it.
- Check warns when an imported cell defines `request` or `ws` (it would
  own HTTP routing / the WebSocket port).
- `self_call` counts only ports this process listens on; a failed guard's
  source is not sent to clients; 204 / 304 carry no Content-Type; a quote
  inside `{…}` in a call argument gets the escaping hint.
- Performance: a lambda's captured lists and maps are no longer copied
  per element — `|> map` / `filter` / … over a captured list is linear
  (was quadratic).
- `[native]`: an Int `/` stays exact after the BigInt re-run (it truncated
  the docs' midpoint example); a handler returning its String parameter
  compiles; an Int overflow inside a Float/Bool expression is kind `range`.
- `sort_by` puts NaN last; an unclosed CSV quote is an error; `quantile`
  refuses q outside [0, 1]; blank CSV cells are missing values for
  `sum_by` / `avg_by`; operator chains count toward the nesting limit.
- `"s" |> map(f)` and `"s".map(f)` raise a type error; kinds
  `rate_limited` / `too_many_requests` answer 429.
- WebSocket: a returned `response(…)` sends its body; error bodies hide
  private handler names; binary frames are answered with an error.
- Clearer errors for reserved words used as JSON fields (`d["cell"]`) and
  for guard locals of a handler that may take a guarded edge.
- Soundness: `"C".delegate(…)` / `"C" |> delegate(…)` are seen by the
  termination and size proofs; a `let` hiding a slot no longer lends the
  slot's bound; the `latency` bound counts retries (think `timeout` now
  covers them), tool rounds, `sleep`, and is advisory with approve / file
  I/O; an interpolated transition target is treated as computed.
- `[native]`: an exact Int / Int with operands past 2^53 is exact; stdin
  read before a BigInt re-run is replayed.
- New `ipow` (exact Int power) and `from_csv(text)`; `read_csv` takes
  `delimiter` and refuses unknown options; `map`/`filter`/… on a non-list
  raise a type error; `format("%.2f", Int)` is exact; `to_float` past the
  Float range raises `range`; `pow` of a non-number raises.
- In an invariant, `slot.get(key)` reads the stored value on Map- and
  record-valued slots too: write-once invariants on `Map<String, Map>` /
  `List<Map>` slots were not enforced.
- approve() shows control characters as escapes (a model could redraw the
  prompt); every reply shape is measured against max_tokens and the budget,
  a reply with no text raises kind `llm`; a `set_budget` reached from a tool
  call can only lower the caller's budget; `..;/` is refused by capability
  path scopes.
- A List `delete` is checked by size invariants only (as verify says);
  `require … else budget` answers 400.
- A JSON request body or bus event with more than 1 000 000 values is
  refused before parsing (a 15 MB line became 2.4 GB in memory).
- Events sent after a linked peer disconnected are logged NOT delivered;
  `soma run` warns when an `emit` meant for `[peers]` goes nowhere.
- Check warns on `m.size ?? default` (`.size` is the entry count, never
  `()`); a negative `[native]` buffer index is reported as written.
- Every path-taking builtin (`load`, `include`, `load_template`,
  `read_files`, `par_read_files`, `word_count`) refuses a `..` segment:
  `load("templates/" + name)` could serve any file.
- The bus port opens only for `[peers]`, `[bus] accept`, `scale` or
  `--join`: an in-process `emit` no longer exposes the program's listeners.
- Kinds `unauthorized` / `unauthenticated` answer 401; error bodies no
  longer name private handlers; the loopback Host check parses the whole
  authority and refuses duplicate Host headers.
- `SOMA_LLM_MOCK=fixed:` over max_tokens raises kind `llm` like a scripted
  mock; guard locals bound after a transition to another state are accepted.
- The file write guard is case-insensitive; `self_call` recognises every
  spelling of this machine; `Origin: null` / `file://` writes are refused
  on loopback servers.
- LLM replies are measured as well as counted: an under-reported reply
  cannot pass max_tokens or the proven cost bound.
- File builtins refuse `..` segments and writes over the program, its
  configuration or storage; a loopback server refuses foreign Host headers
  and cross-origin writes; an HTTP call to the server itself answers
  `self_call`; tool capabilities match query spaces encoded.
- get_status & co. from a machine-less cell with several machines, and
  variant literals with wrong fields, are check errors; the secret recipe
  fails closed; security notes in the serving docs.
- `pow_mod` work is bounded (bits(exp) × bits(m) ≤ 2^30); `to_int` /
  `parse_int` refuse text past the 2^24-bit Int cap.
- A bus event that reaches no peer is logged as NOT delivered; the docs
  give an outbox + acknowledgement pattern for transfers between processes.
- An Int holds at most 2^24 bits: products, `shl` and `product()` past it
  raise kind `range` before they are built (interpreter and native);
  `matmul` is capped at 10^9 multiply-adds.
- `substring` and `range` refuse wrongly typed arguments (kind `type`).
- A huge declared Content-Length no longer aborts `soma serve` (tiny_http
  vendored with a chunked drain; bodies past 256 MB are refused 413).
- A scripted `mock think` reply longer than max_tokens raises kind `llm`.
- Bus peers have bounded send queues (a peer that stops reading is
  disconnected); WebSocket clients are also dropped past 64 MB queued; the
  WebSocket Origin check refuses userinfo and control characters.
- A split string as the last statement of a loop body is a check error;
  disjunctive invariants over the written value are proven.
- Duplicate or list-valued Content-Length is refused and the connection's
  pipelined bytes never run; request-log lines escape control characters.
- Unary operator chains count toward the nesting limit; a List slot's
  invariant `key` is the Int index; `html()` with header pairs passes the
  arity check; guard variables bound in a loop body are recognised.
- `emit` arity mismatches are check errors; a computed Float is refused by
  an Int parameter; CSV rows of empty cells survive a round trip and
  duplicate CSV headers are refused.
- Parameter types are checked all the way down (HTTP, bus, tools, calls);
  a Float past 2^53 is not coerced into an Int parameter; `[native]`
  handlers and mocks are held to the face's return type.
- `from_json` refuses undeclared `_type`s; misspelled builtin types and
  extra builtin arguments are check errors; `()` is refused where a nested
  scalar is declared; `write_csv` keeps rows holding an empty String.
- The equal-clause proof tolerates callees that cannot write the slot;
  contextual keywords work as statement variables.
- A handler parameter may not take a slot's name; face parameter types must
  match the handler's.
- verify and the guard rule see every / after ticks of machine-less cells;
  instance ids are Strings or Ints (`()` refused); a renamed machine is
  reported under `soma run` too.
- `soma run` treats any non-data token as a handler name when several
  exist; NaN / lambdas in responses are valid JSON.
- `[native]` `bnot` is exact past 2^63; native `loop_bound` errors keep
  their kind; `soma run … request` decodes the path like serve.
- Guards of transitions taken from machine-less cells are checked;
  `--strict` fails when `[verify] cells` names no cell of the file; `for`
  over any finite value terminates; an `emit` in an imported file opens the
  bus.
- The equal-clause proof applies only to pure clauses (locals, arithmetic,
  `??`, `.get` of the written slot) with a plain key — other slots,
  nondeterministic or external reads between the require and the write
  made it prove refused writes.
- Invariant slot references are counted deep (interpolation, lambdas,
  match arms) by the checker and the runtime; verify hints never name
  size / key / value; undefined functions in properties are check errors.
- `mock Cell.handler` stubs that cell's handler only; mocks naming nothing,
  test helpers shadowing program handlers or builtins, and test cells with no
  assertion are errors; mocks reach `[native]` sibling calls.
- `--record` logs calls that raised; `describe` / the dashboard keep
  `except` on `*` edges; `soma fix` exits 1 when errors remain.
- A `require` that is exactly an invariant clause over the written value
  proves it (monotone versions); `value` / `key` / `size` outside an
  invariant are check errors; slots in nested lambdas are not function values.
- `soma serve` refuses to start on a damaged database (`quick_check`); a
  file that is not a database is a clean error, not a panic.
- A materialized `range()` is capped at 10M elements; dotfiles under
  static/ are not served; Content-Length + Transfer-Encoding is a 400.
- Quant builtins enforce `max_obs` / `max_assets` and a confidence `alpha` in
  (0, 1); no false "share the path" warning for emit listeners.
- The invariant prover is exact for Ints past 2^53 (intervals widen
  outward instead of rounding), counts think() tools and computed delegates
  among a handler's callees, and does not prove a bare slot read that may be
  `()`.
- `http_get` in a loop, lambda or tool no longer makes a token bound
  advisory (only a latency bound); a saturated cost peak is advisory.
- `[native]` `i64::MIN / -1` raises instead of returning a rounded Float;
  guarded transitions in lambdas see the handler's locals; refinement paths
  keep their parentheses.
- Statements inside block lambdas, `try { }`, if-expressions and match arms
  (`slot[k] = v`, `emit`) are seen by GET→405, the listener set, the
  termination graph, the size prover and guard / invariant purity.
- `return` inside a block lambda is a check error; a bare `return` returns
  `()`; `soma run … request GET "/a?x=1"` splits the query; `every 90m`
  names the duration units.
- `x |> h` with a bare handler name is a call for every analysis
  (termination, cost, route ownership, GET→405, model-driven recursion).
- `subscribe()` dispatches only events the program emits or `[bus] accept`
  lists; invariants and guards may not read files, print or read stdin.
- `.field` on a String or a List raises kind `type`; serve lists only real
  endpoints; `[native]` literal arithmetic that overflows i64 promotes to
  BigInt as interpreted code does.
- Capability-scoped tools cannot load files (`load`, `include`,
  `par_read_files`, …) or `link()`; invariants are pure conditions (no
  handler call or effect builtin, however hidden); a tool calling back its
  think() handler is a termination ⚠; `link()` makes a handler POST-only;
  `load()` substitutes in one pass; a reply over its `max_tokens` raises.
- Errors in imported files are reported in that file.
- `??` short-circuits; `with()` stores BigInt values; strict Float bounds
  after a `require` are proven; `tools_allowed` must name tools.
- Interpolation segments are analysed exactly where the runtime finds them:
  `"{ { expr } }"` ran `expr` unseen by route ownership, termination, cost,
  GET→405, guard and invariant checks.
- A returned String is sent raw only when it is valid JSON text; JSON
  answers carry `nosniff`. `[native]` string literals unescape `{{`/`}}`.
  Face return types are checked element-wise.
- An interpolation segment holding a block (`"{if c { 1 } else { 2 }}"`)
  ends at its matching brace; `.size` on a record without that field is `()`;
  `from_json` checks a declared variant's shape (a missing or mistyped field
  raises kind `type`).
- A stack overflow is not caught by `try`; lambdas handing function values to
  map/filter are a termination ⚠; a computed delegate in `request` owns the
  cell's handlers; face tools are not HTTP endpoints.
- Storage escapes map keys starting with `__` (no refusal, no forgery);
  `()` is false in every condition; invariants must be Bool.
- Floats print shortest digits; `format("%e")`; 5 redirect hops;
  `f(a)(b)` refused; unknown handler names in `soma run` are errors.
- Storage refuses map keys and slot keys starting with `__` (reserved by
  the encoding: a client body could come back as a forged variant, and such
  keys escaped len and size invariants).
- A block lambda's last statement runs when it is not an expression (an
  `emit` there was dropped); conditions and predicates take Bools; route
  ownership follows calls through other cells; `run --fresh` refuses to wipe
  a directory a `soma serve` is using; verify names machines per cell.
- A lambda call counts toward the recursion guard (self-application aborted
  the process); calling a function value is a termination ⚠.
- Cost sees think() in index/field assignments; the guard rule and route
  ownership see UFCS, interpolation, require and emit chains.
- A bare slot name never reaches another cell's storage (it bypassed the
  owner's invariant); writes to slots with non-size invariants no longer
  count the slot (bulk loads were quadratic).
- `mock think` token accounting is capped at max_tokens.

## 2.5.1 — 2026-09-18

A hardening release: seventeen more fresh-agent cycles (realistic ports —
billing, support agents, reservations, analytics, safety interlocks, kanban,
eMAR, a game economy, a WMS — each paired with an adversarial round). Every
finding is fixed or listed as open in docs/agent-ux/LEDGER.md. Highlights:
false proofs closed (require/guards/while conditions in cost and termination,
NaN on Float slots, deletes, shadowed builtins), security fixes in serve
(route ownership, CSRF through cross-cell calls, SSE/WebSocket isolation and
injection, slow-client and flood DoS, capability SSRF, private slots, record
forgery), crypto builtins for authentication, and exact BigInt arithmetic
across builtins.

- Cost: think() in a while condition counts per iteration; the last
  `max_rounds` wins. Route ownership sees UFCS, interpolation and delegate
  calls (auth bypass).
- Storage: an Any slot gives back the String it stored (no JSON re-parse).
- `else { if … }` is a value; exhaustiveness ignores refutable sub-patterns;
  BigInt-exact clamp / pow_mod, 64-bit limits raise; native len counts
  characters; invariants are per cell; qualified-call arity is checked.
- verify models transitions from a machine-less cell (single-machine
  programs); the composition lint flags only callees that swallow failures.
- serve: `emit` is not pushed to WebSocket clients (only `publish` is; SSE
  clients get an emit only when they name it); each WebSocket client has its
  own queue (a slow client is dropped instead of starving the others);
  websocket, tick and bus threads have the 64 MB handler stack (deep
  recursion there aborted the process).
- `try { … }?` re-raises; a guarded match arm does not cover its variant;
  `"{slot}"` works; `{…}` must be one expression.
- `mod` / `idiv` / `floor_div` / `div_round` take Ints; `sum_by` / `avg_by`
  refuse non-numbers; `distinct` keeps values of different kinds.
- serve: a GET that calls into another cell's handler (bare, UFCS or pipe)
  is 405 like a qualified call.
- Crypto builtins take Strings only (`secure_eq("null", ())` was true);
  `random_token` is nondeterministic for replay.
- Check: a test rule calling transition() in a multi-machine program; foreign
  slots through `Cell["slot"]` and interpolation.
- Security: a cell's slots are private (reading another cell's slot by bare
  name is a check error); another cell's handler never replaces a builtin
  (a library's `escape_html` disabled escaping); `soma install` refuses
  dependency names with `/` or `..`.
- New builtins: `sha256`, `hmac_sha256`, `random_token`, `secure_eq`.
- `use` imports each file once (cycles and diamonds load); `soma run` takes
  everything after the handler as arguments; `--json` is always JSON.
- `response()` honours an explicit Content-Type; `html()` takes headers;
  properties see the rules' `let` fixtures.
- Check: a handler cannot be named after a safety builtin (`transition`,
  `approve`, `fail`, `think`…) — it replaced the builtin program-wide while
  verify still proved the edges; one `initial:` per machine.
- verify: a reactive machine (no terminal state, every state returns to the
  initial one) passes --strict.
- serve: one scheduler per data directory (a second serve doubled ticks).
- Capitalised field names in assignments and record literals.
- A panic inside a builtin is a catchable error (kind `internal`); matrix
  builtins cap each dimension (a zero dimension bypassed the size cap).
- Native: i64-to-BigInt local assignment compiles; mixed Float/String
  returns are a check error. Regexes are compiled once per pattern;
  `read_files` is in name order; `format("%d", inf)` raises.
- serve: WebSocket and SSE pushes carry the data as one-line JSON (a String
  payload could forge envelope fields and SSE events for other clients).
- `range` near i64::MAX ends (the step wrapped); `format("%.Nf")` past 1000
  decimals is a `range` error, not a crash; `round`/`floor`/`ceil` of 2^63.
- `soma test`: a test cell's own helpers win its bare calls.
- Security: `&&`/`||`, `ensure`, match guards and properties take Bools (a
  list mask or a String passed compound invariants, guards and asserts);
  tool capabilities match host and path separately and refuse `..`,
  userinfo and fragments; a tool's scope holds inside the agents it calls.
- Agents: `map("tools_allowed", [...])` restricts the tools one think()
  offers; a timeout is not retried (it billed up to 4 × max_tokens past the
  proven bound); a literal transition in a tool keeps think-isolation;
  `recall` works across processes; unknown think() options are check errors.
- Replay records only top-level calls and uses soma.toml [agent].
- Check: a function used as a value, `()` as a slot key, Bool arithmetic and
  Rust keywords in `[native]` code.
- serve: an SSE client receives only the streams it subscribed to (every
  client received every publish); a connection flood that broke the HTTP
  worker pool exits the process (status 70) instead of leaving it alive and
  deaf; a literal `delegate` to a missing handler is a check error.
- Prover soundness: calls in `require` conditions and details are analysed
  (termination, cost, invariants); transition guards must be pure; a delete
  voids a key-exists size proof; Float-slot writes that may be NaN are
  runtime-checked.
- serve: a handler `request` calls is reachable only through `request`
  (auth checks written as `if`/`starts_with` were bypassable); one builtin
  call builds at most 10^8 elements (a single request aborted the process);
  `sleep` bounded; bus lines capped at 16 MB; cross-process `emit` sent at
  commit; `[bus] accept` filters outbound links too.
- Data: `next_id()` ids stay unique across a failing `try`; sum-typed
  parameters and generic variant fields are checked; `write_csv` quotes
  Lists, headers and numeric Strings; exact Int vectors, matrices and
  `median`; date builtins bounded; `for` over `()` runs zero times.
- Check: reading another cell's slot, `every 0ms`, and a native local
  reassigned to another type are errors with a fix.
- A bare call inside a cell to a handler name another cell also defines runs
  the calling cell's own handler (it ran the other cell's).
- `soma serve` no longer exposes the start-up hook (`init` / `start`) as an
  HTTP endpoint; static text files get real content types.
- `soma verify` always ends with a verdict line, prints check errors on
  stdout, and says which `require` would prove an open invariant.
- Native: a buffer passed to a sibling is a check error; Int-valued calls in
  Float expressions compile; a constant overflow is the `range` error.
- `soma verify` fails when `[verify] cells` names a misspelled cell or one
  without a state machine (every property was silently skipped).
- serve refuses a request body carrying `_status` (a handler echoing it let
  the client pick the status and headers), answers 500 for a status outside
  100–599, 400 for a non-UTF-8 body, and keeps `%ZZ` literal.
- The prover narrows by `if` branches and accepts update loops over a
  slot's keys; `cost { tokens }` is stated as reply tokens.
- Data safety: persistent List slots enforce `size` invariants on push;
  state machines persist without a persistent slot; match arms are scopes
  (a failed guard deleted the outer variable).
- Prover soundness: NaN, shadowing by match/lambda/nested lets, growth and
  cost through other cells and emits, termination with re-bound parameters.
- serve: GET/HEAD to a state-changing handler is 405; `_type`/`_variant`
  in client JSON is refused; neither `start` nor `init` is an endpoint.
- `soma run` refuses a program that fails check; native check runs the code
  generator; native shifts, sqrt_int, sb_push_char fixed.
- serve: only `response()`/`html()`/`redirect()` maps are HTTP responses (a
  client map with `_status` stored or echoed forged status, headers and XSS);
  CR/LF header values dropped; the bus port refuses HTTP and private events.
- Prover: a `require` proves only the writes after it; reassignments in match
  arms, `try` and if-expressions; `every`/`after` writers; termination through
  pipes, qualified self-calls and tick loops.
- Hints for `require` without `else`, `and`/`or`/`not`, a quote inside `{…}`;
  CI builds Linux and Intel macOS binaries for every release tag.

## 2.5.0 — 2026-09-18

The agent-experience release. Nine cycles of fresh AI agents (none had seen
Soma) built services, ported Python/Java/TypeScript/Go/Ruby programs, ran
data jobs and LLM pipelines, and attacked the prover, the runtime and the
parser — learning only from the website. Every finding below was reproduced,
fixed, and pinned by a regression test; the whole repository (1,405 `.cell`
files) was re-run through check / verify / test after every batch.

### Soundness and atomicity

- A handler with persistent slots is **one SQLite transaction**: a process
  killed mid-handler (`kill -9`) leaves nothing of it on disk (writes used to
  commit one statement at a time). `every` / `after` ticks are rolled back
  when they raise, like handlers.
- Slots give back exactly what they stored: an Int beyond 64 bits stayed a
  String; records keep their field order; `.keys` / `.values` are sorted.
- Slot value types are enforced on every write (`Map<String, Int>` refuses a
  String, `1.5` or `1.0`; `Map<String, Pay>` refuses a plain map).
- List slots: `rows[i] = v`, `rows[i].f = v`, `rows.delete(i)` used to be
  silently dropped.
- The prover no longer issues false ✓: a `require` in a loop that may run
  zero times, bindings that shadow a narrowed name, interval overflow past
  2^53, termination without a lower-bound base case, cost bounds that skipped
  `every` blocks and `delegate`, liveness that assumed guards pass (now said).
- The prover proves more: a `require` counts for the writes of its own block,
  early exits (`if n >= 1 { return … }`), `require a + b <= K` on the written
  expression, one slot's invariant chained through a local, `size` /
  `len(slot)` invariants, deletes against value invariants. Every ⚠ says why.
- `request` is never an HTTP endpoint (a GET could run a POST route);
  path segments are percent-decoded; CORS on every response.
- A bare call to a name two cells define is a check error (it resolved at
  random); `*` edges no longer fire from states a program stopped declaring.

### Language and builtins

- `format(fmt, …)` (printf subset), `div_round` (HALF_UP), `floor_div`,
  `mod`, `divmod`, `to_fixed`, exact `round(x, d)`; dates: `parse_date`,
  `add_days`, `add_months`, `days_between`, `months_between`,
  `days_in_month`; `chr`, `ord`; `stdev`/`variance` are sample statistics
  (`pstdev`/`pvariance` population); `sin cos tan atan atan2`, `regex_*`,
  `read_stdin`, `write_str` in interpreted handlers too.
- HTTP client: `http_get/post/put/patch/delete` with a default 30 s timeout,
  `headers`, and `{error, kind, status, body}` on failure (the upstream body is
  kept); mockable in tests.
- `think_json` raises kind `json` on a non-object reply; mocked `think`
  costs tokens (budgets testable offline); `trace()` survives requests under
  `soma serve` and records the system prompt.
- Variants round-trip through `to_json` / `from_json`; `soma run` and
  `soma serve` answer valid JSON (NaN/inf → null).
- Literals `1_000_000`, `0xFF`, `0b101`, `"\u{1F600}"`; negative range
  patterns; bare state names; keyword field names (`j.state = …`).
- `|> map`, `|> filter` and `xs[i]` are linear (a 20k-row job went from
  104 s to 0.1 s).

### Toolchain

- `soma check` catches what used to fail at run time: native-only
  primitives outside `[native]`, what the native codegen refuses (buffer
  re-binding, list returns, stepped ranges, literal `/ 0`), `break` outside
  a loop, `transition()` arity, values thrown away (`let j = 1 2`), rules
  outside a test cell, empty test cells, writes to a loop copy (warning),
  `emit` with no listener (warning). 10 000 nested blocks no longer crash it.
- `soma verify --strict` repeats the ⚠ lines by the verdict and always ends
  with one verdict line; orphan `[verify]` properties fail.
- `soma test --json` records carry rule, message, left/right, raised;
  `assert_fails … matching` matches the kind too; `mock` works for any
  handler, `Cell.handler`, `now`, and builtins such as `http_post`.
- `soma serve`: `--no-schedule`, an `llm:` start-up line, endpoints listed,
  whitespace-padded JSON bodies accepted, stored-data audit at start-up
  (undeclared states, invariant violations, re-typed values, renamed slots)
  — also under `soma run`.
- `soma run --fresh`; `.soma_data/` lives beside the program; exact big-Int
  CLI arguments. `soma fix` repairs `;`, `=>` arms, `-> T` on handlers,
  `null`/`True`. `soma describe --json` lists sum types.

### Site and docs

- New `docs/operations.md` (failure modes, statuses, exit codes, migration
  guide, environment variables); serving/guarantees/reference corrected
  against the binary; builtins regenerated from the compiler; the landing
  page links status, guarantees and serving, and states what verify proves.
- The `soma init` starter and the corpus exemplars pass `--strict`.

## 2.4.0 — 2026-09-18

An audit release: the verifier and the runtime were attacked with adversarial
programs, a differential interpreter/native harness and a fuzzer; everything
found is fixed here, each fix with a regression test (274 Rust tests, and
all 1,083 `.cell` programs of the repository re-run with no regression).

### Language

- **`7 / 2` is `3.5` everywhere.** `[native]` handlers used to truncate
  (`3`). Native `/` on two Ints is now a Float; an exact quotient is still an
  Int (BigInt-exact); a slot that can only hold an Int refuses a non-exact
  quotient with a runtime error instead of truncating. `idiv(a, b)` is the
  integer quotient on every backend (now supported in `[native]`, and
  BigInt-exact in the interpreter, where it used to turn any value beyond
  i64 into 0). `soma fix f.cell --native-idiv` migrates old code.
- `soma check` now rejects: a string literal dangling after `return`
  (`return "a" "b"` silently returned `"a"`), a memory invariant naming
  several slots (every write would have been rejected at runtime), a
  non-exhaustive `match` inside a lambda. It now warns on: a call that
  resolves to a builtin instead of the homonymous handler, unreachable code,
  Int / Int in a `[native]` handler.

### Soundness

- Memory invariants: `delete` bypassed `size` invariants; `soma verify` did
  not see bracket writes (`slot[k] = v`) or deletes.
- Termination proof: mutual recursion, recursion hidden in an operand
  (`1 + f(n + 1)`) and decreasing recursion without a base case were all
  reported as "structurally terminate".
- Cost proof: "`tokens` bound proven" was claimed for a `think()` reached
  through a sibling handler, a lambda, or a loop over a list of unknown
  size. Calls are now composed; unknown counts make the bound advisory.

### Runtime

- **Security:** `soma serve` served `/static/../soma.toml` (API keys), the
  `.cell` sources and `.soma_data`. Static files are confined to `static/`.
- `[native]`: a division by zero aborted the whole process (SIGABRT); a
  panic in native code is now an ordinary, `try`-catchable error. Concurrent
  native builds in one directory poisoned the dylib cache (wrong results,
  silently): builds take a lock, publish atomically, and every dylib carries
  a build id verified at load.
- `soma test` ignored `[agent]` / `[models]` in `soma.toml` (so `mock` had no
  effect); `soma replay` blamed nondeterminism when the source had changed;
  `soma run f.cell -7` rejected negative arguments.

### For agents

- `soma init` creates `app.cell` (the name every doc uses — it used to write
  `main.cell`), a starter that passes check/verify/test, and an `AGENTS.md`.
  The stdlib is embedded in the binary: a fresh project no longer warns
  `unknown property 'persistent'`.
- `soma docs agent|reference|gotchas|builtins|all` — embedded, offline.
- `soma example <terms…>` searches the verified corpus; `soma example <id>`
  prints a program's source.
- soma-lang.dev is generated by `tools/build_site.py`: `llms-full.txt`,
  `builtins.json`, `gotchas.json`, `corpus/index.json` (316 programs, each
  re-verified at build time), `agent.md`, real 404s, open CORS.

### Examples

- `rebalancer`: `POST /approve` reached the `approve()` builtin, not the
  handler — the human approval gate was unreachable through `request`.
  `examples/atlas`: same shadowing in `verdict`. `examples/padovan.cell`
  checked against wrong expected values.

MIT license added.
