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
endpoint.** A handler is private when its name starts with `_`
(`on _debit(account, amount)`), or when it lives in another cell. Put domain
logic in its own cell and keep the HTTP cell thin. `soma check` warns when a
handler and one of `request`'s routes share a name.

Only the cell that defines `request` is routed. Other cells are reachable from
it by calling their handlers by name.

## Requests and responses

`request` receives `(method, path, body)` — and a fourth `query: Map`
argument when it declares one. The declared type of `body` decides its shape,
identically under `soma serve`, `soma test` and `soma run`:

- `body: String` — the raw request text; `from_json(body)` parses a JSON body
  (it raises kind `json` on invalid JSON: wrap it in `try`).
- `body: Map` — the JSON object (or form fields) already parsed; a request
  whose body is not JSON is answered `400 {"kind": "json"}` before the handler
  runs; an empty body is `map()`.

A handler may return:

| Return value | HTTP |
|---|---|
| a Map or a List | `200`, JSON |
| a String | `200`, text |
| `response(status, body)` | that status; the value is `{_status, _body}` — assert `r._status == 404` in tests |
| `html(body)` / `html(status, body)` | HTML |
| `redirect(url)` | `302` |

An uncaught error is a `500` with `{"error": …}`. Map error kinds to statuses:

```soma
let r = try { _hold(id) }
if r.kind == "not_found"          { return response(404, map("error", r.detail)) }
if r.kind == "invalid_transition" { return response(409, map("error", r.detail)) }
if r.error != ()                  { return response(422, map("error", r.detail)) }
```

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

No TLS, no authentication, no header access from handlers, no rate limiting.
It binds 127.0.0.1 (`--host 0.0.0.0` to expose it), plus `PORT + 2` for the signal bus and `PORT + 1` for WebSockets when a cell declares `on ws`. Every response carries `Access-Control-Allow-Origin: *` (browsers on any origin may call it; put a proxy in front to restrict).
Run it behind a reverse proxy and firewall the bus port.
