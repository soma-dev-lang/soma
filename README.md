# soma

A fractal, declarative language for verified distributed systems.
Built for AI agents. The compiler is the collaborator.

```
soma serve app.cell -p 8080                        # one node
soma serve app.cell -p 8081 --join localhost:8082   # cluster
```

## Install

```bash
git clone https://github.com/soma-dev-lang/soma.git
cd soma/compiler && cargo build --release
sudo cp target/release/soma /usr/local/bin/
```

## Quick start

```bash
soma init myapp && cd myapp
soma serve app.cell          # http://localhost:8080
soma check app.cell          # verify contracts
soma fix app.cell            # auto-repair errors
soma verify app.cell         # prove state machines
soma lint app.cell           # catch anti-patterns
```

## Agent workflow

```
generate  ->  fix  ->  check  ->  verify  ->  serve
```

`soma fix` auto-repairs missing handlers, contradictory properties. `soma lint` catches redundant `to_json`, unchecked `.get()`, if-chains that should be `match`. The compiler does the work.

## What makes Soma different

| | Kubernetes | Erlang | Akka | Soma |
|---|---|---|---|---|
| Distribution model | External YAML | In the VM | In the library | **In the language** |
| Verified before deploy | No | No | No | **Yes (CTL + CAP)** |
| Same code local/cluster | No | Almost | Almost | **Yes, zero changes** |
| Agent-first tooling | No | No | No | **Yes (fix, lint, describe)** |

## Pattern matching

Soma has pattern matching that rivals Rust and Elixir:

```soma
on request(method: String, path: String, body: String) {
    let req = map("method", method, "path", path)
    match req {
        {method: "GET", path: "/"}                   -> home()
        {method: "GET", path: "/api/" + resource}    -> list(resource)
        {method: "POST", path: "/api/" + resource}   -> create(resource, body)
        {method: "DELETE", path: "/api/" + resource}  -> delete(resource)
        _ -> response(404, map("error", "not found"))
    }
}
```

Map destructuring, string prefix patterns, guard clauses, or-patterns, range patterns, variable binding -- all composable.

## Storage

Auto-serializes. No `to_json`/`from_json` needed:

```soma
memory { users: Map<String, String> [persistent, consistent] }

users.set("alice", map("name", "Alice", "score", 95))
let user = users.get("alice")
print(user.name)   // "Alice" — auto-deserialized
```

## For AI agents

- **Agent guide**: [AGENT.md](AGENT.md) -- syntax, do/don't, verification patterns
- **Language reference**: [SOMA_REFERENCE.md](SOMA_REFERENCE.md) -- full syntax
- **JSON output**: `soma check --json`, `soma describe`, `soma lint --json` -- machine-readable
- **Vision**: [VISION.md](VISION.md) -- roadmap for intent compilation, diagnostic agents, live verification

## Docs

- **Website**: [soma-lang.dev](https://soma-lang.dev)
- **Paper**: [Scale as a Type](https://soma-lang.dev/paper)
- **Examples**: `examples/` -- pricing engine, job queue, chat, mini kubernetes, 100+ more

## License

MIT
