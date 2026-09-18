# `soma serve` — exactly what gets exposed, and how

```
soma serve app.cell -p 8080          # HTTP on :8080
soma run   app.cell request GET /stats ""     # call the router with no server
```

## Routing: three rules, in this order

1. `GET /static/<file>` serves `<project>/static/<file>` — confined to that
   directory (`..` cannot escape it).
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
`{"error": "kind: detail", "kind": kind}`: `not_found` → 404; `guard_failed`,
`forbidden`, `approval_required` → 403; `invalid_transition`, `conflict` →
409; `invariant`, `ensure` → 422; `json`, `type`, `division_by_zero` and your
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
match, so match on `method` for the ones that write: with CORS open to every
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
  is dropped after 1024 queued events). Both are sent AT COMMIT: a handler
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
code; you do pay for it in throughput, and a `think()` call holds the line
for as long as the model takes.

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
bus) is opened only when a cell uses `emit`, declares `scale`, lists events
in `[bus] accept`, or `--join` is given — the start-up log says `bus:
listening` or `bus: not started`. The bus speaks one line per event,
`EVENT <name> <json>\n` (a line past 16 MB closes the connection); a
receiver runs only the events its program emits itself or lists in
`[bus] accept = ["reading"]` in soma.toml, and never `request`, `ws`,
`start`/`init` or a `_private` handler. An `emit` goes to every connected
peer AT COMMIT (a handler that raises sends nothing). An incoming bus event
waits for the process-wide handler lock like a request (keep handlers short
on busy links); `start()` runs BEFORE the links to `[peers]` are up, so an
`emit` there reaches no other process (use `after 2s { … }`). A peer that
restarts is reconnected when both processes list each other in `[peers]`;
with a one-way link, restart the sender too, or re-`--join`; `PORT +
1` only when a cell declares `on ws`. Every response, static files, the
dashboard and the pre-handler 400s included, carries
`Access-Control-Allow-Origin: *` (browsers on any origin may call it; put a
proxy in front to restrict). `--no-schedule` starts the HTTP side without the
`every` / `after` threads (tests, debugging). Run it behind a reverse proxy
and firewall the bus port.
