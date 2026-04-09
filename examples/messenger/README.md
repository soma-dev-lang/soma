# Soma Messages

A WhatsApp-class messenger built on Soma.

```sh
soma serve examples/messenger/app.cell -p 8080
open http://localhost:8080
```

Then open the same URL in a second browser window (or incognito tab),
sign in as a different user, and chat with yourself in real time.

## What's in the box

| Feature                       | How                                                              |
|-------------------------------|------------------------------------------------------------------|
| 1:1 messaging                 | `POST /send` with a 1:1 thread id (auto-canonicalised)           |
| Group messaging               | `POST /thread/new` with > 2 members                              |
| Persistent history            | `[persistent, consistent]` memory → SQLite, zero config          |
| Read receipts                 | `state message_lifecycle { sent → delivered → read }`            |
| Typing indicators             | `emit typing_indicator` → SSE push, no storage                   |
| Online presence               | `[ephemeral, local]` presence slot + heartbeat GC every 30s      |
| Real-time delivery            | `sse("new_message", "message_status", …)` + `EventSource` client |
| Web client                    | `static/index.html` — vanilla HTML+CSS+JS, no frameworks         |

The whole server is **one Soma cell** (`app.cell`, ~310 lines including
comments). The whole client is **one HTML file** (~430 lines including
inline CSS and JS, no build step). That is the entire codebase.

## Why it's interesting

### The message lifecycle is formally verified

`app.cell` declares:

```soma
state message_lifecycle {
    initial: sent
    sent -> delivered
    delivered -> read
    * -> deleted
}
```

`soma verify examples/messenger/app.cell` runs the V1.3 refinement
check on the handler bodies and proves three things:

1. Every `transition()` call in every handler names a state declared
   above. A typo like `transition(id, "raed")` would be a compile error.
2. Every state in the lifecycle is reached by some handler.
3. Per-handler effect summary, with path conditions:

```
✓ refinement: handler `mark_delivered` ⟶ {delivered [if msg.status == "sent"]}
✓ refinement: handler `mark_read` ⟶ {delivered [if msg.status == "sent"], read}
✓ refinement: handler `delete_msg` ⟶ {deleted}
```

The spec and the code can no longer drift apart. This is the WOW
feature from `docs/SEMANTICS.md §1.5` working on a real cell.

### The cluster topology is in the source

```soma
scale {
    replicas: 5
    shard: messages
    consistency: strong
    tolerance: 2
    cpu: 1
    memory: "256Mi"
}
```

`soma verify` proves the CAP picture (CP mode, 3/5 quorum, tolerates
2 node failures). The same source runs on 1 machine
(`soma serve app.cell`) or N machines (`soma serve app.cell --join
host:port`). No deploy YAML, no Helm chart, no Terraform.

### HTTP, persistence, pub/sub, and real-time are built in

- **HTTP routing**: `on request(method, path, body)` with pattern
  matching. No Express, no Flask.
- **Persistence**: `[persistent, consistent]` memory → SQLite. No
  Postgres driver, no migrations file.
- **Real-time**: `sse(...)` returns a server-sent-events response;
  `emit event_name(payload)` pushes to every connected client. No
  Socket.IO, no Redis pub/sub.
- **Auto-serialization**: `messages.set(id, to_json(msg))` and
  `from_json(messages.get(id))` — no schema registry, no Avro.

The cell IS the system. Compare the line count to the equivalent
in any mainstream stack.

## API

```http
POST /register           {"username": "alice", "display_name": "Alice"}
POST /thread/new         {"creator": "alice", "members": ["alice","bob"]}
POST /send               {"from": "alice", "thread": "alice:bob", "text": "hi"}
POST /read               {"message_id": "...", "by": "bob"}
POST /delivered          {"message_id": "..."}
POST /delete             {"message_id": "..."}
POST /typing             {"thread": "alice:bob", "user": "alice", "is_typing": true}
POST /presence           {"user": "alice", "status": "online"}

GET  /users
GET  /threads/<user>     — user's threads with unread counts, sorted by recency
GET  /thread/<id>        — thread metadata + full message history
GET  /events             — SSE stream (new_message, message_status, typing_indicator,
                           presence_change, thread_created)
GET  /                   — web client
```

For 1:1 chats the thread id is the canonical sorted-username form
(`alice:bob`), so it's the same regardless of who initiated. Groups
get a generated id like `g_1738000000_42`.

## Smoke test

```sh
SOMA=./compiler/target/release/soma

# 1. Compile-time checks (✓ check, ✓ verify, ✓ refinement)
$SOMA check  examples/messenger/app.cell
$SOMA verify examples/messenger/app.cell

# 2. Start the server
$SOMA serve  examples/messenger/app.cell -p 8080 &

# 3. Register two users
curl -s -X POST localhost:8080/register \
     -d '{"username":"alice","display_name":"Alice"}'
curl -s -X POST localhost:8080/register \
     -d '{"username":"bob","display_name":"Bob"}'

# 4. Send a message
curl -s -X POST localhost:8080/send \
     -d '{"from":"alice","thread":"alice:bob","text":"hello bob"}'

# 5. Fetch the thread
curl -s localhost:8080/thread/alice:bob

# 6. Subscribe to the event stream (Ctrl-C to stop)
curl -N localhost:8080/events
```

## Architecture diagram (in text)

```
              ┌─────────────────────────────────────┐
              │   Web client (static/index.html)    │
              │   vanilla HTML+JS, EventSource SSE  │
              └──────────────┬──────────────────────┘
                             │
                  HTTP + SSE  │
                             ▼
              ┌─────────────────────────────────────┐
              │            Messenger cell           │
              │                                     │
              │   face   :  HTTP routes via         │
              │             on request(...)         │
              │                                     │
              │   memory :  users, messages,        │
              │             threads, presence       │
              │             [persistent,consistent] │
              │             → SQLite, transparent   │
              │                                     │
              │   state  :  message_lifecycle       │
              │             VERIFIED by V1.3        │
              │             refinement check        │
              │                                     │
              │   scale  :  5 replicas, CP, q=3     │
              │             same source, N nodes    │
              │                                     │
              │   every 30s: presence GC            │
              └─────────────────────────────────────┘
```

## Limitations (intentional)

- **No end-to-end encryption.** This is a transport-and-state demo,
  not Signal. E2E would need a key-exchange protocol modelled as a
  state machine of its own — interesting future work.
- **No phone-number verification.** Sign in is `username + display_name`.
  In production you'd plug in OTP via SMS.
- **No media (images / voice / video).** Add a `[persistent]` blob
  store and an upload endpoint when you need them; the cell won't
  fight you.
- **No mobile clients.** The web client is the demo. The HTTP+SSE
  API is the same for any client.

## Files

```
examples/messenger/
├── app.cell             # the entire server (~310 lines)
├── soma.toml            # manifest + verify config
├── static/
│   └── index.html       # the entire client (~430 lines)
└── README.md            # this file
```

That's the whole project.
