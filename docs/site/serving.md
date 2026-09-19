# `soma serve` — exactly what gets exposed, and how

```
soma serve app.cell -p 8080          # HTTP on :8080
soma run   app.cell request GET /stats ""     # call the router with no server
```

## Routing: three rules, in this order

1. `GET /static/<file>` serves `<project>/static/<file>` — confined to that
   directory (`..` and symlinks cannot escape it; dotfiles such as `.env`
   are never served). A request carrying both Content-Length and
   Transfer-Encoding, or a repeated / non-numeric Content-Length, is refused
   (400); a declared body past 256 MB is refused (413) before it is read.
2. A path that the cell's `request(method, path, body)` handler **matches
   explicitly** — a literal (`"/stats"`) or a prefix pattern (`"/hold/" + id`)
   in one of its `match` arms, or a path it tests (`path == "/reset"`,
   `starts_with(path, "/wipe/")`) — goes to `request`.
3. Otherwise, if the first path segment is the name of a **public handler** of
   the request-owning cell, that handler is called with the remaining segments
   and query values as arguments: `POST /add/5` → `add(5)`. This is how an HTML
   form posts to `/add`. Anything else goes to `request` (or 404 without one).

**Every public handler of the request-owning cell is therefore an HTTP
endpoint** — except `request` itself, which is only ever the router, and
the handlers `request` calls (directly or through its own helpers): those
are reachable ONLY through `request`, so the checks it makes before calling
them (an Authorization header, a method) cannot be walked around with
`POST /wipe/a`. A handler is private when its name starts with `_`
(`on _debit(account, amount)`), or when it lives in another cell. Put domain
logic in its own cell and keep the HTTP cell thin. `soma check` warns when a
handler and one of `request`'s routes share a name.

Only the cell that defines `request` is routed. Other cells are reachable from
it by calling their handlers by name — not by reading their slots:
`Store.config.get(k)` from another cell is a check error (a cell's memory is
its own; add a handler to Store that returns the value).

## Requests and responses

`request` receives `(method, path, body)` — plus `query: Map` and
`headers: Map` when it declares them, bound BY NAME (a 4th parameter called
`headers` gets the headers, anything else the query): `on request(method:
String, path: String, body: Map, query: Map, headers: Map)`. Header names
are lower-case (`headers.authorization`); a repeated header is one entry,
its values joined with ", " (a repeated QUERY key keeps its last value). `OPTIONS`
requests are answered 204 with permissive CORS headers before any handler. A trailing `Map` parameter may be left out by a
caller (it is `map()`), so a test still calls `request("GET", "/x", "")`.
The declared type of `body` decides its shape:

- `body: String` — the raw request text; `from_json(body)` parses a JSON body
  (it raises kind `json` on invalid JSON: wrap it in `try`).
- `body: Map` — the JSON object (or form fields) already parsed; a request
  whose body is not JSON is answered `400 {"kind": "json"}` before the handler
  runs; an empty body is `map()`. `soma run` parses its text argument the
  same way; in a test cell pass the Map itself (`request("POST", "/x",
  map("a", 1))`) — a String there is a `type` error, not parsed.

A handler may return:

| Return value | HTTP |
|---|---|
| a Map or a List | `200`, JSON |
| a String or a number | `200`, `{"result": …}` (JSON); a String that is valid JSON text (`to_json(x)`) is sent as that JSON |
| `()` | `200`, `null` |
| `response(status, body)` | that status; the value is `{_status, _body}` — assert `r._status == 404` in tests |
| `html(body)` / `html(status, body)` | HTML |
| `redirect(url)` | `302` |

An error the handler does not catch is answered by its kind, as
`{"error": message, "kind": kind}` (the message names the refusal, e.g. `require failed: MyTag: …`, `guard failed for transition a → b`): `not_found` → 404; `unauthorized`,
`unauthenticated` → 401; `rate_limited`, `too_many_requests` → 429; `guard_failed`,
`forbidden`, `approval_required` → 403; `invalid_transition`, `conflict` →
409; `invariant`, `ensure` → 422; `json`, `type`, `date` (parse_date / add_days on a bad date), `range`, `division_by_zero` and your
own `require … else Tag` / `fail("tag")` → 400; `stack_overflow`, `llm`,
`budget`, undefined names → 500 (the full table is in operations.md). Map a
kind yourself only when you want a different status or body:

```soma
let r = try { _hold(id) }
if r.kind == "invalid_transition" { return response(410, map("error", r.detail)) }
if r.error != ()                  { fail(r) }      // re-raise: the default mapping answers (under serve; a test sees the raised error)
```

Path segments reach `request` percent-decoded (`/stock/a%20b` → `"/stock/a b"`),
except an encoded slash: `%2F` stays `%2F`, so a value cannot fake a path
segment (`split(rest, "/")` sees the segments the client meant).
Path patterns hold ONE variable, at the end (`"/loans/" + rest`); split
`rest` for more segments, or take the rest from the body or query. Public
handlers (no `_` prefix, `request` aside) are also reachable directly at
`/<handler>/<arg>/…`: arguments are coerced to the declared parameter types
(`/decide/x/true` → Bool), a trailing `Map`/`List` parameter takes the JSON
body (a non-JSON body → `400 {"kind": "json"}`). At start-up `serve` calls a
zero-argument `start()` and `init()` handler (each one the cell has); neither
name is an HTTP endpoint (a request could re-run it). A handler that changes
state — a slot write, a transition, an emit, a call into another cell —
answers `GET`/`HEAD` with `405` (`Allow: POST`) — for the auto-exposed
`/<handler>` endpoints; routes of your own `request` answer every method you
match (a HEAD request reaches `request` with method `"HEAD"` — match it next
to `"GET"` if clients send it), so match on `method` for the ones that write: with CORS open to every
origin, a GET that writes is writable by any web page (`<img src=…>`). A JSON
body (or a Map-typed path/query argument) carrying `_type`, `_variant` or
`_values` anywhere is refused (400), whatever the type of `body` — also for
`body: String`: a client cannot forge a record or a sum-type variant.
`from_json` on other text that names a declared variant checks its shape
(a missing or mistyped field raises kind `type`).

## Realtime: WebSocket, SSE, events

- `on ws(msg: String)` receives each text frame on port+1
  (`ws://127.0.0.1:<port+1>`). Its return value is sent back to that client:
  a String as-is, a Map/List as JSON, `()` sends nothing. A raise answers
  `{"error": …, "kind": …}` like HTTP. The handler is atomic and rolled back
  like any other; `ws` is not an HTTP endpoint. A browser connection is
  accepted only from a localhost / 127.0.0.1 Origin (and from the Host's own
  origin only when serving beyond loopback with `--host`).
- `publish("stream", data)` is pushed to every WebSocket client as
  `{"event": "stream", "data": …}` and to the SSE clients subscribed to that
  name; an `emit ev(data)` is cell-to-cell: it reaches only SSE clients that
  NAME it (`sse("ev")`), never WebSocket clients (a client that stops reading
  is dropped after 1024 queued events (a WebSocket client also at 64 MB
  queued) and the drop is logged — so a stalled SSE subscriber can hold
  1024 × the size of one event in memory (100 KB events: ~100 MB each): keep
  events small, or put a proxy with its own buffering in front; a bus peer that stops
  reading is disconnected after 1024 queued events). Both are sent AT COMMIT: a handler
  that raises (or a `try` that rolls back) pushes nothing. WebSocket clients
  have no per-client routing: EVERY one receives every `publish`, so do not
  publish one user's or tenant's data where others hold a WebSocket — give
  each an SSE stream of its own.
- SSE: a `request` route returns `sse("stream1", "ev")`; the client receives
  only the named streams (`sse()` with no name: every `publish` stream) — so per-tenant
  streams (`sse("t_" + tenant)`) behind your own auth check in `request` keep
  tenants apart. The first event is `connected`. There is no replay — after
  a reconnect, re-fetch state.
- A handler that some `emit` targets is an event listener, not an HTTP
  endpoint (a client could forge the event); `publish` counts as a state
  change (GET → 405).

## Concurrency and atomicity

Each request runs on its own thread, and **top-level handler invocations are
serialized**: one handler at a time, process-wide. A handler that raises is
rolled back (writes and transitions). You do not need locks or compensation
code; you do pay for it in throughput, and in a plain handler a `think()` call
holds the line for as long as the model takes.

### `[task]` handlers: think outside the lock

`on review(d: Doc) [task] { … }` runs as **steps**: each `think()` ends the
current step (its writes commit), waits for the model outside the lock, then a
new step starts. Concurrent requests overlap their model calls (200 × 2 s
mocked calls finish in about 9 s instead of 400 s). Rules:

- A failure rolls back the **current step only**; steps before the last
  `think()` stay committed. A `try` around a `think()` cannot undo writes made
  before that `think()` (check warns: write after the think instead).
- Anything read before a `think()` may have changed after it: read, `require`
  and write in the step after the last `think()` (check warns when a slot is
  read before and written after). The prover carries no fact across a
  `think()` in a `[task]` handler; such writes are checked at run time.
- Only the entry point decides. A `[task]` handler called from a plain handler
  — `on request(…)` routing to it, an `emit` listener, another cell — runs
  inside that caller's atomic unit and holds the lock; mark the caller
  `[task]` too (`on request(method: String, path: String, body: String) [task]`).
  Ticks take it after the period: `every 1min [task] { … }`, `after 5s [task] { … }`.
  Check warns when a plain handler or tick calls a `[task]` handler.
- A `[task]` handler without `think()` is one atomic unit, like a plain one;
  `[task]` and `[native]` exclude each other.
- `SOMA_LLM_MOCK_LATENCY_MS=2000` gives the mock a latency, to test the overlap.

### Hordes: one `[task]` handler over many inputs

```soma
on audit(docs: List) {
    return horde(Reviewer.review, docs, map("concurrency", 200,
        "on_result", "_store", "on_done", "_summarize", "max_attempts", 2))
}
on _store(v: Map) { verdicts.set(v.id, v) }      // verdicts: [persistent, immutable]
```

`horde()` returns an id (`Audit:h1`) at once; a pool of `concurrency`
workers (default 8, at most 1000) runs the target once per input. Poll
`horde_status(id)`, read `horde_results(id)`, stop with `horde_cancel(id)`.

- **Persisted queue.** The horde and its tasks are written in the caller's
  transaction (a rolled-back caller starts nothing) to soma.db — under serve and
  run always, even without a `[persistent]` slot. A restarted server resumes
  unfinished hordes. With several servers on one `.soma_data`, each horde runs
  in one of them (a lease renewed every second); when that server stops,
  another takes it over about 6 s later.
- **Written at the call.** The target and the options are part of the call:
  `horde(Reviewer.review, docs, map(…))` with literal option names and
  literal handler names. Check refuses a computed target or an options map
  from a variable or a request (an HTTP client could otherwise pick the
  handler or drop the budget); name callbacks `_store` (a public callback is
  an HTTP endpoint too — check warns). Only the owner cell (and test rules)
  can read or cancel a horde.
- **Exactly once, where it matters.** A task's result is recorded, and
  `on_result` called, in the same atomic unit as the target's last step: after
  a `kill -9`, a task is either recorded or run again, never recorded twice.
  Steps before its last `think()` may run again — keep them idempotent or
  write only in the last step.
- **Failures.** A task that raises is retried (at the back of the queue) up to
  `max_attempts` (default 1), then `on_error(input, error)` runs — `error` is
  the Map a `try` gives, `{error, kind, detail}` — and the task counts as
  failed. `on_done(id)` runs once when nothing is left, cancellation included.
- **Deploys.** A restart resumes unfinished hordes only if the target and the
  callbacks still exist with the same arities; otherwise it prints why and
  leaves the tasks waiting (restore the handler, or cancel the horde).
- **Status.** `running` counts tasks waiting for the rate limiter too; right
  after `horde_cancel` in the same handler the state is `cancelling`.
- **Rate limits.** `[agent] rpm` / `tpm` in soma.toml (or `SOMA_LLM_RPM` /
  `SOMA_LLM_TPM`) bound every `think()` of the process, workers included: at
  most `rpm` requests and `tpm` tokens in any 60 s window. A 429 from the
  provider pauses every caller.
- **Budget.** `map("budget_tokens", 2000000)` is a hard ceiling for the whole
  horde: before each `think()` of a task (or of `on_result` / `on_done`) the
  runtime reserves an upper bound of the call — the request's bytes (a token
  is at least a byte) plus its `max_tokens` — and settles the real count after.
  A call that does not fit yet waits, outside the lock, for calls in flight to
  settle; one that can never fit is refused (kind `budget`) and the horde stops
  (`state: "exhausted"`; the tasks it stopped count as cancelled, without
  `on_error`). Measured: 600 tasks, 100 in flight, budget 10 000 → exhausted at
  9 900–10 000, never above. The ceiling assumes the provider honors
  `max_tokens` (one that does not is detected, kind `llm`, and charged), and it
  counts settled calls: calls in flight at a `kill -9` reached the provider and
  are sent again after the restart — up to `concurrency` × (request +
  `max_tokens`) more than the counter shows.
- **Nested hordes.** A horde started inside a task or a callback of a
  budgeted horde runs under that budget too: the whole tree cannot spend past
  the root's ceiling. `on_done`'s `think()` counts as well — keep headroom for
  it (it fails, kind `budget`, on an exhausted horde).
- **Cost proof.** `soma verify` prints each horde's bound: a literal
  `budget_tokens`; else inputs × per-task cost × `max_attempts` when the inputs
  are a literal list or range; else unbounded. In a cell with
  `cost { tokens: N }`, a horde without a literal `budget_tokens` over inputs
  of unknown size makes the bound unprovable (a check error).
- **Rounds.** For a simulation, `map("snapshot", world, "apply", "_apply",
  "seed", 7, "instance", "id")`: the target takes `(input, snapshot)`, so every
  agent of the round sees the same world; `apply` runs at the end, in input
  order, once per task — the result does not depend on which reply came back
  first; `on_done` may start the next round. `seed` makes `random()` in task
  *i* draw the same numbers on every run; `instance` names the input field
  that identifies an agent — its `remember()` / `recall()` and its model
  conversation carry over to its next round. Measured: 10 000 agents × 20
  rounds in 46 s, identical on two runs.
- **Votes.** `vote(Judge.check, claim, 5)` asks 5 agents the same thing and
  returns `{winner, count, k, unanimous, errors, votes}`; inside a `[task]`
  step under serve / run the calls run at once.
- Without workers (`soma test`) a horde runs right after the calling handler
  commits — its next statements (mapping the id to a batch, …) run first, as
  under serve — one task per unit; `soma run` waits for its hordes before
  exiting.
- Measured: 10 000 tasks against a 2 s mocked model at concurrency 500 in 41 s;
  a `kill -9` after 2 500 of them, then a restart: 10 000 results, each once.

## Storage

`[persistent]` slots live in `<project>/.soma_data/soma.db` (SQLite). `soma
run` uses the same database, so state carries over between runs; `soma test`
uses fresh in-memory storage every time.

## What `soma serve` does not do

No TLS, no built-in authentication (read `headers.authorization` in
`request` and refuse; make tokens with `random_token()`, store
`hmac_sha256(secret, password + salt)`, compare secrets with `secure_eq`;
cookies arrive in `headers.cookie` and are set with
`response(303, "", "Location", "/", "Set-Cookie", "sid=…; HttpOnly; SameSite=Strict")`
or `html(200, page, "Set-Cookie", …)`; a repeated form or query field keeps
its last value; the `/__soma/` dashboard is unauthenticated — firewall it), no rate limiting, no cap on open connections (one
thread each: at the machine's thread limit the process exits with status 70
so a supervisor restarts it — cap connections in the reverse proxy).
It binds 127.0.0.1 (`--host 0.0.0.0` to expose it). `PORT + 2` (the signal
bus) is opened only when soma.toml lists `[peers]` or events in `[bus]
accept`, a cell declares `scale`, or `--join` is given (an `emit` alone
stays in this process: no port) — the start-up log says `bus:
listening` or `bus: not started`. The bus speaks one line per event,
`EVENT <name> <json>\n` (a line past 16 MB, or holding more than a
million JSON values, closes the connection); a
receiver runs only the events its program emits itself (with `[peers]` or a cluster) or lists in
`[bus] accept = ["reading"]` in soma.toml, and never `request`, `ws`,
`start`/`init` or a `_private` handler. An `emit` goes to every connected
peer AT COMMIT (a handler that raises sends nothing). An incoming bus event
waits for the process-wide handler lock like a request (keep handlers short
on busy links); `start()` runs BEFORE the links to `[peers]` are up, so an
`emit` there reaches no other process (use `after 2s { … }`). Each
`[peers]` link is re-established when it drops (a peer that restarted, was
down at start-up, or was cut off for reading too slowly); events emitted
meanwhile are logged NOT delivered, not queued; `PORT +
1` only when a cell declares `on ws`. Every response, static files, the
dashboard and the pre-handler 400s included, carries
`Access-Control-Allow-Origin: *` (browsers on any origin may call it; put a
proxy in front to restrict). `--no-schedule` starts the HTTP side without the
`every` / `after` threads (tests, debugging). Run it behind a reverse proxy
and firewall the bus port.

## Security notes for handlers

- **Loopback servers** (the default bind) refuse a request whose `Host` names
  another site (DNS rebinding) and a state-changing request whose `Origin`
  is another site (a cross-site form POST) — 403. With `--host` the server is
  public: authenticate every writing route.
- **`http_*` with a client URL** reaches whatever the URL names (SSRF): allow
  only known hosts. A call to this very server (its HTTP, WebSocket or bus
  port) answers `kind: "self_call"` at once — a handler cannot call its own
  endpoints (the handler lock is held); call the handler directly.
- **`http_*` results**: a success returns the body itself (a Map, List or
  String); a failure returns `{error, kind, status, body}`. Test
  `type_of(r) == "Map" && r.status != ()` before reading `r.kind`.
- **Templates**: `render` / `render_each` / `load` substitute values as they
  are — wrap client text with `escape_html(…)`; `html()` does not escape
  either.
- **`redirect(url)`** sends whatever URL it is given: redirect only to paths
  you build (`"/orders/{id}"`), never to a client-supplied URL.
- **CSV exports** keep cells as written: a cell starting with `=`, `+`, `-`
  or `@` is a formula to a spreadsheet — prefix client text with `'` when
  the file is meant for one.
