# Operating a Soma service — what happens when it goes wrong

Tested facts about the process, not intentions. Version: `soma --version`
(the website's `/version.json` says which release is published; a served
program has no such route).

## What kills the process, what does not

| Event | Effect |
|---|---|
| A handler raises (require, invariant, transition, `fail`, division by zero, a String reaching an `Int` parameter) | That request is answered with a 4xx/5xx JSON body (table below); every write and transition of the request is rolled back; the process stays up |
| Runaway recursion in a handler | Answered `500 {"kind": "stack_overflow"}` at depth 512; request threads have a 64 MB stack so the guard fires before the OS does; the process stays up |
| A `[native]` handler panics (a `buf_get` past the end, an `idiv` by zero) | Caught at the boundary: an ordinary `try`-catchable error with the same kind the interpreter would raise (`index` → 400, `division_by_zero` → 400); the process stays up. Int overflow does not panic: it promotes to BigInt exactly as interpreted code does |
| `think()` fails or times out | Kind `llm`, rolled back like any error; the request is answered, not hung: one provider round-trip is capped at 60 s (`timeout_ms` in the options map, or `SOMA_LLM_TIMEOUT_MS`), retried up to 3 times on 429/5xx |
| A scheduled `every` / `after` block raises | Logged, rolled back (writes and transitions — it is a handler invocation), next tick runs |
| Port already answering, `soma check` errors, an unreadable file | `soma serve` refuses to start and exits 1 — it never serves a program that does not check (`--no-check` overrides) |
| Out of memory, SIGKILL, `kill -9`, SIGTERM | The process dies at once (no draining: an in-flight client gets an empty reply); committed handlers are in `.soma_data/soma.db` (SQLite); the handler in flight is lost as a whole — its writes sat in one uncommitted SQLite transaction |
| Disk full while writing | Expected (not exercised): the SQLite write fails, the request is rolled back and answered 500 |
| A slow upstream (`http_post` to a service that hangs) | The handler holds the process-wide lock for the whole call: every other request waits. Every http builtin has a timeout (default 30 s; pass `map("timeout", ms)`) — keep it short; the upstream is not cancelled |
| A slow handler (a quadratic loop, a huge `to_json`, a loop of 100 000 `slot.set`) | Handlers run one at a time: every other request and every scheduler tick WAITS for it — there is no per-request time limit. `soma verify` proves termination, not speed. A persistent slot write costs about 1 ms (each is an SQLite statement): a 100 000-key rebuild in one handler holds the process for ~2 minutes. Keep handlers short; batch bulk loads outside the request path; put a proxy timeout in front |
| The program changed and `.soma_data/` is older | A renamed slot is a new empty slot (the old rows stay in the file); a slot whose value TYPE changed gives back the old values with their old type; an invariant added later is not checked against stored values (verify proves it for future writes only); a state-machine instance stored in a state the new machine no longer declares takes no transition, `*` edges included. `soma serve` and `soma run` audit the database at start-up and print one `warning: stored data: …` line per problem: instances in undeclared states, values an invariant refuses (List slots and `size` clauses included), values of another type than declared, a slot re-declared List ↔ Map. `soma serve` (which owns its data directory) also reports slot data no slot declares any more and tables of cells the program no longer declares (renamed/removed) — `soma run` does not, since several programs often share one directory. Migrate (below) or delete `.soma_data/` |

## Addresses and ports

- `soma serve app.cell -p 8080` binds **127.0.0.1:8080**. Nothing is reachable from the network until you pass `--host 0.0.0.0` (or put a reverse proxy in front — recommended: TLS, auth and headers are the proxy's job; Soma reads none).
- The event bus for `emit` between processes binds port **+2** (8082), the WebSocket endpoint port **+1** (8081) only when a cell declares `on ws(...)`. Both follow `--host`.
- `serve` probes the port first: a process already answering there is an error (exit 1), not a silent bind beside it.
- The dashboard is `/__soma/` on the same port; static files are served from `./static/` only.

## Exit codes

| Command | 0 | 1 |
|---|---|---|
| `soma check` | no errors (warnings allowed) | at least one error |
| `soma verify` | `VERIFY OK` (vacuous when the program has no state machine — it says so) | `VERIFY FAILED …` (also when `soma check` fails) |
| `soma test` | every assertion passed | any failure, or `soma check` fails, or no test cell |
| `soma run` | the handler returned | the handler raised, or the program does not check, or the handler name is unknown |
| `soma serve` | (runs until stopped) | cannot start: port taken, check errors, bind failure |
| `soma deploy` | provider CLI succeeded | the CLI is missing or failed (the Dockerfile is still generated) |
| `soma docs`, `soma example` | printed | unknown topic / no match |

`--json` on `check`, `verify`, `test`, `describe`, `example` prints machine-readable stdout (also for a fatal error such as an unreadable file); diagnostics go to stderr. `soma verify --strict` turns every ⚠ into a failure.

## HTTP answers for a raised error

Body: `{"error": "<message>", "kind": "<kind>"}`.

| kind | status | raised by |
|---|---|---|
| `not_found` | 404 | `fail("not_found", …)` |
| `guard_failed`, `forbidden`, `approval_required` | 403 | a transition guard; `fail("forbidden")`; `approve()` with nobody to answer |
| `invalid_transition`, `conflict` | 409 | `transition()` off the machine; `fail("conflict")` |
| `invariant`, `ensure` | 422 | a memory invariant refusing a write; `ensure` |
| `json`, `division_by_zero`, `type`, `index` | 400 | a non-JSON body for `body: Map`; arithmetic; a wrong-typed argument or a value that does not fit the slot's declared type; a list index out of range (also from a `[native]` buffer) |
| your own `require … else Tag` / `fail("tag", …)` | 400 | the program refused the request |
| `stack_overflow`, `llm`, `budget`, `undefined_variable`, `undefined_function`, `no_handler` | 500 | the program itself is wrong or the world failed |

A handler that returns normally answers 200 with its value as JSON (`()` is `null`, a String is `{"result": "…"}`), or the status inside a `response(status, body)` map.

## Limits

- Int is arbitrary precision (i64 fast path, BigInt beyond); Float is f64; `7 / 2` is `3.5`.
- Recursion depth: 512 frames, then `stack_overflow`.
- Request bodies and paths: no configured cap; 20 MB bodies and 20 KB paths were served without crashing. Put the cap on the proxy.
- Handlers run one at a time (a process-wide lock): correct under contention, no parallelism inside one process. Throughput is not a goal.
- `forall` properties in tests walk every value up to 20 000, then sample with a fixed seed.
- One process, one SQLite file (`.soma_data/soma.db`, created beside the program); no replication unless a `scale` section and a bus join are configured (experimental).

## Migrating stored data

There is no migration command: a migration is a handler, run once with
`soma run` against the same `.soma_data/` (it shares the database with a
running `soma serve`, and like every handler it is one transaction).

- **Added slot / field**: old rows read `()`. Backfill in a one-shot handler
  (`for id in skus.keys { if prio.get(id) == () { prio.set(id, 2) } }`), or
  default at read time (`prio.get(id) ?? 2`).
- **Renamed state**: run a copy of the program that still declares the old
  state and an edge out of it (`picked -> packed`), with a `_migrate` handler
  calling `transition(id, "packed")` for each stuck id; then serve the new
  program. `soma run migrate.cell _migrate`.
- **Renamed slot**: keep the old slot declared next to the new one for one
  release, copy in `_migrate`, then drop it. The audit names the orphaned
  rows until then.
- **Re-typed slot**: read, convert, `set` — or rename the slot.
- **Tightened invariant**: stored values that violate it are served but can
  not be written back; fix them in `_migrate` or keep the old bound.

Make `_migrate` idempotent (check before writing) and back up
`.soma_data/soma.db` first.

## Environment variables

| Variable | Effect |
|---|---|
| `SOMA_LLM_KEY` (or `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`) | the provider key for `think()`; without one `soma test` mocks and `soma serve` raises kind `llm` |
| `SOMA_LLM_MOCK=echo` \| `fixed:<text>` | `think()` never reaches a provider (overrides `[agent] mock` in soma.toml); `soma serve` prints `llm: MOCK …` at start-up when the program calls think |
| `SOMA_LLM_TIMEOUT_MS` | one provider round-trip cap (default 60 000) |
| `SOMA_APPROVE=always` \| `never` | answers `approve()` when no terminal is attached (`soma serve` fails closed otherwise: 403 `approval_required`) |
| `PORT` | not read — pass `-p` |

## Between processes

`emit` reaches every cell of the same process synchronously. Across
processes it needs the bus: a `[peers]` table in soma.toml
(`other = "host:PORT+2"`) on the sending side, and on the RECEIVING side
the events it accepts from other processes: `[bus] accept = ["paid"]` (an
event this program emits itself is accepted too; anything else — any
other handler — is refused). The event reaches every cell with `on paid`. `--join host:bus-port`
registers a node for `scale` sharding; it does not by itself forward `emit`.
A peer that is down when the process starts is logged as
`peer: … failed` and not retried — start the receiving process first. This
is the experimental corner of Soma; single-process is the supported shape.

## Persistence

`[persistent]` slots and state-machine instances live in `.soma_data/soma.db` next to the `.cell` file (wherever the command is run from), shared by `soma serve` and `soma run`; `soma run --fresh` deletes it first. `soma test` starts from empty storage every run. Back up the file; there is no migration tool — a renamed slot is a new, empty slot. Values round-trip exactly: an Int beyond 64 bits comes back as that Int, a variant as a variant, `()` as `()`. A write that does not fit the slot's declared value type (`Map<String, Int>` given a String or `1.0`; `Map<String, Pay>` given a plain map) is refused with kind `type` before it commits; an Int written to a `Float` slot is stored as a Float. One process holds one connection to the database and each handler runs inside `BEGIN IMMEDIATE … COMMIT`, which also serializes handlers across processes.

## Deploying on Linux

There is no published Linux binary yet; build from source (Rust stable + GMP):

```sh
apt-get install -y build-essential libgmp-dev m4 git
git clone --depth 1 --branch v<version> https://github.com/soma-dev-lang/soma
cd soma/compiler && cargo build --release
# ./target/release/soma serve /srv/app/app.cell -p 8080 --host 0.0.0.0
```

`soma deploy --target fly|cloudflare|aws` generates a multi-stage Dockerfile that does exactly this (build stage `rust:1-bookworm`, runtime `debian:bookworm-slim` + `libgmp10`) and invokes the provider CLI; a missing CLI exits 1 after generating the files. A systemd unit is a one-liner: `ExecStart=/usr/local/bin/soma serve /srv/app/app.cell -p 8080`, `WorkingDirectory=/srv/app` (the database lives there), `Restart=always`.

## Logs

One line per request on stderr: `POST /pay/x → 409 2ms invalid transition …`. Start-up prints the cell, its handlers, the database path, the bind address and the dashboard URL. No log files are written by Soma; use the supervisor's.
