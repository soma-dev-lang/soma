# http_client

Retries with backoff and pagination over `http_get` / `http_post`. A failure is a value ({error, kind, status, body}), never a raise.

```soma
use http_client

let user = fetch_retry(url, map("attempts", 4))
if failed(user) { return response(502, map("error", user.kind)) }
post_retry(url, body, map("idempotent", true))
fetch_pages(url, map("items", "data", "next", "next_url"))
with_query(url, map("q", "a b"))  encode_component(s)  backoff_ms(attempt, base, cap)
```

Retried: timeout, refused or broken connection, 408, 425, 429, 5xx. A POST only with `idempotent`. `fetch_pages` raises kind `http` when a page fails. In tests, script the network with `mock http_get [...]`. (The spec calls this package http-client; a package name is an identifier.)

Pure Soma. Its proofs are its tests — re-run them on your toolchain:

```
soma test packages/http_client/http_client.cell
```
