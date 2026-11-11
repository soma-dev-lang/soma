# ratelimit

Fixed windows and token buckets kept in the cell's persistent memory: they survive a restart, and since a handler is atomic two requests never both take the last slot.

```soma
use ratelimit

if !allow("login:" + ip, 5, 60, now()) { return response(429, map("retry_after", retry_after("login:" + ip, 5, 60, now()))) }
take("api:" + key, 100, 10, now(), 1)        // a bucket of 100, 10 back per second
remaining(key, limit, window_s, now())  level(key, capacity, refill, now())  forget(key)
```

Kind: `BadLimit`. The invariants `hits >= 0` and `tokens >= 0` are on the memory.

Pure Soma. Its proofs are its tests — re-run them on your toolchain:

```
soma test packages/ratelimit/ratelimit.cell
```
