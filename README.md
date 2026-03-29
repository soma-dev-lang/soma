# soma

A fractal, declarative language for verified distributed systems.

Same code. One machine or a cluster. The compiler proves it correct.

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
soma verify app.cell         # prove state machines
```

## For AI agents

Soma is designed to be built by agents. The compiler is the supervisor.

- **Agent guide**: [AGENT.md](AGENT.md) — syntax rules, do/don't, verification patterns, builtins
- **Language reference**: [SOMA_REFERENCE.md](SOMA_REFERENCE.md) — full syntax
- **JSON output**: `soma check --json`, `soma verify --json`, `soma describe` — machine-readable

## What makes Soma different

| | Kubernetes | Erlang | Akka | Soma |
|---|---|---|---|---|
| Distribution model | External YAML | In the VM | In the library | **In the language** |
| Verified before deploy | No | No | No | **Yes (CTL + CAP)** |
| Same code local/cluster | No | Almost | Almost | **Yes, zero changes** |

## Docs

- **Website**: [soma-lang.dev](https://soma-lang.dev)
- **Paper**: [Scale as a Type](https://soma-lang.dev/paper)
- **Agent guide**: [AGENT.md](AGENT.md)
- **Language reference**: [SOMA_REFERENCE.md](SOMA_REFERENCE.md)
- **Examples**: `examples/` — pricing engine, job queue, chat, mini kubernetes, 100+ more

## License

MIT
