# queue

Durable jobs with claim / acknowledge: a claimed job is invisible for a while and handed out again if its worker dies; after too many deliveries it is parked as dead. A declared state machine (`ready → claimed → done | dead`), checked by `soma verify`.

```soma
use queue

let id = enqueue("emails", map("to", "a@example.com"))
let job = claim("emails", now(), 30)          // () when there is nothing to do
ack(job.id)                                   // or nack(job.id, 5)
depth("emails")  job_state(id)  dead_jobs("emails")
```

`claim` reads the open jobs in order: for queues of thousands, not millions. Kinds: `BadQueue`, `BadVisibility`, `UnknownJob`.

Pure Soma. Its proofs are its tests — re-run them on your toolchain:

```
soma test packages/queue/queue.cell
```
