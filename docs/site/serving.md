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
   in one of its `match` arms — goes to `request`.
3. Otherwise, if the first path segment is the name of a **public handler** of
   the request-owning cell, that handler is called with the remaining segments
   and query values as arguments: `POST /add/5` → `add(5)`. This is how an HTML
   form posts to `/add`. Anything else goes to `request` (or 404 without one).

**Every public handler of the request-owning cell is therefore an HTTP
endpoint** — except `request` itself, which is only ever the router. A
handler is private when its name starts with `_`
(`on _debit(account, amount)`), or when it lives in another cell. Put domain
logic in its own cell and keep the HTTP cell thin. `soma check` warns when a
handler and one of `request`'s routes share a name.

Only the cell that defines `request` is routed. Other cells are reachable from
it by calling their handlers by name.

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
| a String or a number | `200`, `{"result": …}` (JSON) |
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
zero-argument `start()` (or `init()`) handler when the cell has one; that
start-up hook is not an HTTP endpoint (a GET could re-run it).

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
`request` and refuse), no rate limiting.
It binds 127.0.0.1 (`--host 0.0.0.0` to expose it). `PORT + 2` (the signal
bus) is opened only when a cell uses `emit`, declares `scale`, or `--join` is
given — the start-up log says `bus: listening` or `bus: not started`; `PORT +
1` only when a cell declares `on ws`. Every response, static files, the
dashboard and the pre-handler 400s included, carries
`Access-Control-Allow-Origin: *` (browsers on any origin may call it; put a
proxy in front to restrict). `--no-schedule` starts the HTTP side without the
`every` / `after` threads (tests, debugging). Run it behind a reverse proxy
and firewall the bus port.
