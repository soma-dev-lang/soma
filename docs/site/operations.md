# Operating a Soma service — what happens when it goes wrong

Tested facts about the process, not intentions. Version: see `/version.json`.

## What kills the process, what does not

| Event | Effect |
|---|---|
| A handler raises (require, invariant, transition, `fail`, division by zero, a String reaching an `Int` parameter) | That request is answered with a 4xx/5xx JSON body (table below); every write and transition of the request is rolled back; the process stays up |
| Runaway recursion in a handler | Answered `500 {"kind": "stack_overflow"}` at depth 512; request threads have a 64 MB stack so the guard fires before the OS does; the process stays up |
| A `[native]` handler panics (overflow in a pure-i64 cell, index out of range) | Caught at the boundary: an ordinary `try`-catchable error, `500` over HTTP; the process stays up |
| `think()` fails or times out | Kind `llm`, rolled back like any error; the request is answered, not hung: one provider round-trip is capped at 60 s (`timeout_ms` in the options map, or `SOMA_LLM_TIMEOUT_MS`), retried up to 3 times on 429/5xx |
| A scheduled `every` handler raises | Logged, rolled back, next tick runs |
| Port already answering, `soma check` errors, an unreadable file | `soma serve` refuses to start and exits 1 — it never serves a program that does not check (`--no-check` overrides) |
| Out of memory, SIGKILL, `kill -9` | The process dies; committed writes are in `.soma_data/soma.db` (SQLite); a request in flight is lost as a whole (atomic) |
| Disk full while writing | Expected (not exercised): the SQLite write fails, the request is rolled back and answered 500 |
| A slow handler (a quadratic loop, a huge `to_json`) | Handlers run one at a time: every other request and every scheduler tick WAITS for it — there is no per-request time limit. `soma verify` proves termination, not speed. Keep handlers short; put a proxy timeout in front |
| The program changed and `.soma_data/` is older | A renamed slot is a new empty slot; a slot whose TYPE changed (List → Map) reads as empty while the old rows stay in the database; an invariant added later is not checked against stored values (verify proves it for future writes only); a state-machine instance stored in a state the new machine no longer declares is stuck (`valid_transitions(id) == []`). None of these is reported at start-up: migrate the data or delete `.soma_data/` |

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
| `json`, `division_by_zero`, `type` | 400 | a non-JSON body for `body: Map`; arithmetic; a wrong-typed argument |
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

## Persistence

`[persistent]` slots and state-machine instances live in `.soma_data/soma.db` next to the `.cell` file, shared by `soma serve` and `soma run` in that directory. `soma test` starts from empty storage every run. Back up the file; there is no migration tool — a renamed slot is a new, empty slot.

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
