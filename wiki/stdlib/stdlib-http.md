---
name: stdlib-http
description: HTTP request/response builtins, plus SSE streaming.
type: reference
since: V1.0
related: [handler, face]
---

# Stdlib: HTTP

`soma serve` exposes the cell over HTTP. `on request(method, path,
body)` is the dispatch entry point.

## Receiving requests

```soma
on request(method: String, path: String, body: String) {
    match path {
        "/"            -> html("<h1>Welcome</h1>")
        "/api/data"    -> get_data()
        "/api/" + r    -> get_resource(r)
        _              -> response(404, map("error", "not found"))
    }
}
```

The runtime parses HTTP, builds the call, and serializes the return
value as the response.

## Response constructors

```soma
html("<p>Hello</p>")                          // Content-Type: text/html
response(201, map("id", 1))                    // custom status
redirect("/other")                             // 302 redirect
sse("event_name", "payload")                  // SSE event
map("ok", true)                                // implicit JSON 200
```

Return type matters:

- A `Map` → JSON response with status 200.
- A `String` → text/plain or text/html (depending on whether it
  starts with `<`).
- `response(status, value)` → custom status code.
- `redirect(url)` → 302 with `Location: url`.
- `sse(event, data)` → SSE event in a long-poll stream.

## Outgoing HTTP

```soma
let r = http_get("https://api.example.com/data")           // unbounded
let r = http_get("https://api.example.com", map("max_bytes", 65536, "timeout", 5000))
let r = http_post(url, body)
let r = http_put(url, body)
let r = http_delete(url)
```

All return `String` (the response body). Without `max_bytes` they're
unbounded; with it, the cell's [[budget-proof]] survives.

`http_get` with a `headers` option:

```soma
let r = http_get(url, map(
    "max_bytes", 65536,
    "timeout", 5000,
    "headers", map("Authorization", "Bearer {token}", "Accept", "application/json")
))
```

## Server-sent events

```soma
on stream() {
    every 1s {
        sse("tick", to_json(map("ts", now())))
    }
}
```

`sse(event, data)` emits a single SSE event. The runtime buffers and
flushes per request.

## WebSocket

```soma
on connect() {
    ws_connect("ws://localhost:8081")
}
on message(msg: String) {
    ws_send("echo: {msg}")
}
```

`ws_connect` opens an outgoing WebSocket; `ws_send` sends to the
current incoming connection.

## Examples

A minimal REST API:

```soma
cell Api {
    memory { items: Map<String, String> [persistent, consistent] }

    on request(method: String, path: String, body: String) {
        let req = map("method", method, "path", path)
        match req {
            {method: "GET",    path: "/api/items"}    -> items.values |> map(s => from_json(s))
            {method: "GET",    path: "/api/items/" + id} -> from_json(items.get(id) ?? "{}")
            {method: "POST",   path: "/api/items"}    -> {
                let data = from_json(body)
                let id = to_string(next_id())
                items.set(id, to_json(data |> with("id", id)))
                data |> with("id", id)
            }
            {method: "DELETE", path: "/api/items/" + id} -> {
                items.delete(id)
                map("deleted", id)
            }
            _ -> response(404, map("error", "not found"))
        }
    }
}
```

A live dashboard with SSE:

```soma
cell Dashboard {
    memory { events: Map [ephemeral] }
    every 1s {
        let snapshot = events.values
        sse("update", to_json(snapshot))
    }
    on request(method: String, path: String, body: String) {
        if path == "/stream" { return sse("hello", "{}") }
        if path == "/" { return html(render_html()) }
        return response(404, "{}")
    }
}
```

## Edge cases

- The handler return value is serialized as JSON unless wrapped in
  `html()`, `response()`, `redirect()`, or `sse()`.
- Path matching is by string equality (or string-prefix patterns
  `"prefix" + rest`). No regex.
- `http_get` is synchronous and blocking. No async.
- A failed `http_get` returns `()` (Unit). Always check.

## Related

- [[handler]] — `on request` as a special handler.
- [[face]] — how to declare an HTTP-exposed API contract.
