# soma

The first language where AI agent behavior is formally verified.

`cell agent` + `think()` + state machine = **proven termination**.

```
soma serve agent.cell -p 8080                     # serve agent
soma verify agent.cell                            # PROVE it terminates
soma serve app.cell -p 8081 --join localhost:8082  # cluster
```

## Install

```bash
curl -fsSL https://soma-lang.dev/install.sh | sh
```

Or build from source:
```bash
git clone https://github.com/soma-dev-lang/soma.git
cd soma/compiler && cargo build --release
sudo cp target/release/soma /usr/local/bin/
```

## Quick start

```bash
soma init myapp && cd myapp
soma serve app.cell          # http://localhost:8080
soma fix app.cell            # auto-repair errors
soma verify app.cell         # prove state machines
soma lint app.cell           # catch anti-patterns
```

## Verified AI Agents

```soma
cell agent Researcher {
    face {
        signal research(topic: String) -> Map
        tool search(query: String) -> String "Search the web"
    }

    state workflow {
        initial: idle
        idle -> researching -> analyzing -> done
        * -> failed
    }

    on search(query: String) {
        http_get("https://api.search.com?q={query}")
    }

    on research(topic: String) {
        set_budget(5000)                          // hard token cap
        transition("t", "researching")
        let facts = think("Research: {topic}")    // LLM + tool calling
        transition("t", "analyzing")
        let summary = think("Synthesize: {facts}") // multi-turn context
        transition("t", "done")
        map("summary", summary, "tokens", tokens_used())
    }
}
```

```
$ soma verify agent.cell

✓ no deadlocks
✓ eventually(done | failed)     ← PROVEN: agent always terminates
✓ after(researching, analyzing | failed)
4 passed, 0 failed
```

No other agent framework can prove this.

## Agent Runtime

| Builtin | What it does |
|---------|-------------|
| `think(prompt)` | LLM call with auto tool dispatch + retry |
| `think_json(prompt)` | LLM returns structured Map |
| `delegate(cell, signal, args)` | Cross-agent task dispatch |
| `set_budget(n)` / `tokens_used()` | Hard token cap enforcement |
| `remember(k, v)` / `recall(k)` | Persistent agent memory |
| `approve(action)` | Human-in-the-loop gate |
| `trace()` | Full execution log |

Config: `SOMA_LLM_KEY`, `SOMA_LLM_URL` (OpenAI or ollama), `SOMA_LLM_MODEL`

## Pattern Matching

```soma
on request(method: String, path: String, body: String) {
    let req = map("method", method, "path", path)
    match req {
        {method: "GET", path: "/"}                   -> home()
        {method: "GET", path: "/api/" + resource}    -> list(resource)
        {method: "POST", path: "/api/" + resource}   -> create(resource, body)
        n if n.method == "OPTIONS"                   -> cors()
        _ -> response(404, map("error", "not found"))
    }
}
```

Map destructuring, string prefix, guard clauses, or-patterns, range patterns — all composable.

## Agent Workflow

```
generate  →  fix  →  lint  →  check  →  verify  →  serve
```

- `soma fix` auto-repairs missing handlers, contradictory properties
- `soma lint` catches redundant to_json, unchecked .get(), if-chains
- `soma check --json` returns errors with `kind` + `fix` fields
- `soma describe` outputs rich JSON: handlers, memory, state machines, tools
- `soma verify` proves state machine properties with CTL model checking

## Deterministic record / replay

Production incidents, single-stepped on your laptop:

```bash
soma run --record bot.cell    # writes bot.somalog (JSON-lines, opt-in)
soma replay bot.cell          # bit-deterministic re-execution
```

Each replay entry passes if the live result matches the recorded one.
When a handler calls a nondeterministic builtin (`now`, `random`, …),
the recorder logs the call site and replay reports each divergence
with a suggested fix. Demo: `examples/v1/02_replay_trader.cell`.

## Performance: `[native]` vs Rust and C

`[native]` compiles handlers to a Rust `cdylib` per cell. Same source,
~100–300× speedup over the interpreter on tight numeric loops, and
**essentially tied with hand-written sequential Rust on the CLBG
numeric challenges** (geomean ~1.02×, faster on 3 of 5). The C
reference suite under `bench/clbg_c_ref/` reproduces the same
comparison against `clang -O3 -march=native`. Writeups:
`bench/results/SUMMARY_clbg.md`, `bench/results/clbg_c_vs_rust_vs_soma_raw.txt`.

Soma never returns wrong answers on integer overflow — the dual-mode
dispatch wrapper falls back to GMP (`rug` crate) on i64 overflow.
Numba's `@njit` silently returns garbage in the same situation;
`examples/overflow_corpus/` exercises this on every commit.

## What makes Soma different

| | LangChain | CrewAI | Kubernetes | Soma |
|---|---|---|---|---|
| Agent termination proof | No | No | No | **Yes (CTL)** |
| Tool calling verified | No | No | No | **Compiler-checked** |
| Distribution model | No | No | YAML | **In the language** |
| Auto-repair | No | No | No | **soma fix** |
| Same code local/cluster | N/A | N/A | No | **Yes** |

## For AI agents

- **Agent guide**: [AGENT.md](AGENT.md)
- **Language reference**: [SOMA_REFERENCE.md](SOMA_REFERENCE.md)
- **LLM reference**: [llms.txt](https://soma-lang.dev/llms.txt)
- **Paper**: [Scale as a Type](https://soma-lang.dev/paper)
- **Examples**: `examples/` — agents, pipelines, pricing engine, chat, 100+ more

## Test Suite

111 tests: 89 unit + 19 integration + 3 agent (live LLM via ollama/gemma3),
plus a 100-cell language corpus (`examples/usecases/`) and the 10 CLBG
challenges (`examples/clbg_corpus/`) — 222/222 green at HEAD.

## License

MIT
