# Soma Messages

A WhatsApp-class messenger built on Soma. **One cell. Zero JavaScript application code.**

```sh
soma serve examples/messenger/app.cell -p 8080
open http://localhost:8080
```

Then open the same URL in a second browser context (incognito window
or a different browser) and sign in as a different user. Chat with
yourself in real time. Read receipts, typing indicators, online
presence, group chats — all of it. None of it written in JavaScript.

## What's in the box

| Feature                       | How                                                              |
|-------------------------------|------------------------------------------------------------------|
| 1:1 messaging                 | `_send` handler + canonical thread ids (`alice:bob`)             |
| Group messaging               | `_create_thread` with > 2 members                                |
| Persistent history            | `[persistent, consistent]` memory → SQLite, transparent          |
| Read receipts                 | `state message_lifecycle { sent → delivered → read }`            |
| Typing indicators             | `publish("typing_<thread>", html)` → htmx `sse-swap`             |
| Online presence               | `[ephemeral, local]` slot + 30s GC heartbeat                     |
| Real-time delivery            | `publish("messages_<thread>_<viewer>", html)` per-user channels  |
| Web client                    | **server-rendered HTML + htmx**, no application JS               |

The whole thing is **one Soma cell** (`app.cell`, ~780 lines including
comments and inline HTML/CSS). The "client" is one library — htmx —
loaded once at the top of the page. Every interaction either makes an
htmx request that returns an HTML fragment, or consumes an SSE event
whose payload is also an HTML fragment. There is no application
JavaScript anywhere.

## Why server-rendered

The first version of this messenger had a 430-line vanilla JS client
that maintained its own message cache, presence cache, typing cache,
and render loop. That client was a parallel state machine the V1.3
refinement check **could not verify** — the spec lived in the cell,
the implementation lived in the browser, and they could drift apart
silently. Exactly the bug refinement was supposed to prevent.

The server-rendered version puts every state transition the user can
observe inside a Soma handler. The cell is the system, all the way
to the pixel.

## Why this is interesting

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
✓ refinement: handler `_mark_delivered` ⟶ {delivered [if msg.status == "sent"]}
✓ refinement: handler `_mark_read`      ⟶ {delivered [if msg.status == "sent"], read}
✓ refinement: handler `_delete_msg`     ⟶ {deleted}
```

The spec and the code can no longer drift apart. **This applies to
every state transition the user can observe**, because every state
transition lives in the cell. There is no JS counterpart to verify
separately.

### Real-time delivery via dynamic SSE channels

Each browser tab subscribes to a small set of stable, dynamic SSE
event names like `messages_alice:bob_alice` (alice's view of the
alice:bob thread) and `threads_alice` (alice's sidebar). When a
state-changing handler runs, it calls `publish("messages_alice:bob_alice", html)`
to push the updated HTML fragment to every browser listening for that
exact event name. htmx's `sse-swap` attribute consumes it and swaps
the new fragment into place.

Each event is per-thread, per-viewer. A typical multi-user run
generates events on ~10 distinct stream names, and each browser only
processes the streams for the elements actually on its page.

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

### HTTP, persistence, pub/sub, real-time, AND rendering — built in

- **HTTP routing**: `on request(method, path, body)` with pattern
  matching. No Express, no Flask.
- **Persistence**: `[persistent, consistent]` memory → SQLite. No
  Postgres driver, no migrations file.
- **Pub/sub**: `publish(stream_name, html)` builtin pushes HTML
  fragments to every SSE client subscribed to that name. No Socket.IO,
  no Redis pub/sub.
- **Server-rendered HTML**: `html(200, """ <html>{interpolation}... """)`
  triple-quoted strings with full expression interpolation. No
  templating engine, no React, no SSR framework.

The cell IS the system. There is nothing else.

## API

```http
GET  /                       — login form
POST /register               — register; HX-Redirect → /app/<user>
GET  /app/<user>             — full app shell
GET  /threads/<user>         — sidebar HTML fragment
GET  /thread/<user>/<id>     — chat panel HTML fragment
POST /send                   — send a message; emit SSE; return 204
POST /read                   — mark as read; emit SSE; return 204
POST /delivered              — mark as delivered; return 204
POST /typing                 — broadcast typing; return 204
POST /presence               — update presence; return 204
POST /thread/new             — create a thread; HX-Redirect + JSON body
GET  /events                 — SSE stream for live updates
```

For 1:1 chats the thread id is the canonical sorted-username form
(`alice:bob`), so it's the same regardless of who initiated. Groups
get a generated id like `g_1738000000_42`.

## Multi-user testing

### 1. Two browser windows

The current user is encoded in the URL path (`/app/alice`,
`/app/bob`), so two regular tabs work — no localStorage clash. Even
simpler, use two browser contexts:

- Chrome regular + Chrome incognito
- Chrome + Firefox

Sign in twice as different users and watch messages flow live.

### 2. Curl

```sh
curl -X POST localhost:8080/register -d '{"username":"alice"}'
curl -X POST localhost:8080/register -d '{"username":"bob"}'
curl -X POST localhost:8080/send \
     -d '{"from":"alice","thread":"alice:bob","text":"hi"}'
curl localhost:8080/threads/bob              # sidebar HTML
curl localhost:8080/thread/bob/alice:bob     # chat panel HTML
curl -N localhost:8080/events                # SSE stream
```

### 3. Multi-user simulator

```sh
soma serve examples/messenger/app.cell -p 8080
./examples/messenger/sim.sh
```

The simulator drives three users (alice, bob, carol) through a
realistic conversation: 1:1 chat, typing, read receipts, group
creation, group chat, group read receipts, presence change. Generates
~75 SSE events across ~10 distinct dynamic stream names in a few
seconds. Useful for end-to-end smoke testing without browser
juggling.

## Architecture diagram (in text)

```
              ┌───────────────────────────────────────┐
              │          Browser (htmx only)          │
              │   No application JS. Just sse-swap    │
              │   attributes wired to dynamic event   │
              │   names like messages_alice:bob_alice │
              └──────────────────┬────────────────────┘
                                 │
                  HTTP + SSE      │  every payload is HTML
                                 ▼
              ┌───────────────────────────────────────┐
              │             Messenger cell            │
              │                                       │
              │   on request(method, path, body)      │
              │     → render_login()                  │
              │     → render_app(user)                │
              │     → render_thread_list_inner(user)  │
              │     → render_chat_panel(u, thread)    │
              │     → _send / _mark_read / _typing    │
              │                                       │
              │   memory : users, messages, threads,  │
              │            presence (auto SQLite +    │
              │            in-memory ephemeral)       │
              │                                       │
              │   state  : message_lifecycle          │
              │            VERIFIED by V1.3           │
              │                                       │
              │   scale  : 5 replicas, CP, q=3        │
              │                                       │
              │   publish(stream, html) → SSE bus     │
              │   every 30s: presence GC              │
              └───────────────────────────────────────┘
```

## Files

```
examples/messenger/
├── app.cell             # the entire server + client renderer (~780 lines)
├── sim.sh               # multi-user end-to-end simulator
├── soma.toml            # manifest + verify config
└── README.md            # this file
```

That's the whole project. **No `static/` directory, no JS bundle, no
build step, no `package.json`, no framework.** The cell renders the
shell, the cell renders every fragment, the cell pushes the live
updates. Every state transition the user can observe lives in a
handler that V1.3 refinement has verified.

## Limitations (intentional)

- **No end-to-end encryption.** This is a transport-and-state demo,
  not Signal. E2E would need a key-exchange state machine, which
  Soma's refinement check would happily verify too.
- **No phone-number verification.** Sign in is `username + display_name`.
- **No media (images / voice / video).** Add a `[persistent]` blob
  store and an upload endpoint when you need them.
- **No mobile clients.** The web client is the demo. The HTTP+SSE
  API is the same for any client.
