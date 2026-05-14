---
name: think
description: Bounded LLM call as a first-class builtin. Enables compile-time budget proofs.
type: concept
since: V1.0
related: [handler, ensure, think-isolation, budget-proof, stdlib-agent]
---

# `think()`

`think(prompt)` is Soma's first-class LLM call. It performs a model
invocation, auto-dispatches tool calls declared in the cell's
[[face]], and returns the result as a string. With `think_json` the
return is a structured `Map`.

The single most important property: **`think` is bounded by default**.
When called with a `max_tokens` option, the [[budget-proof]] checker
proves a closed-form upper bound on its memory consumption.

## Syntax

```soma
let answer = think("What is 2 + 2?")
let data = think_json("Return as JSON: {prompt}")

// Bounded form — enables compile-time budget proofs
let answer = think("prompt", map(
    "max_tokens", 500,
    "timeout", 10000           // milliseconds
))
```

Without the options map, the [[budget-proof]] downgrades the cell to
*advisory* (the bound exists but the checker prints the call site).
With it, the bound is proven.

## How tool dispatch works

When the cell is `cell agent` and the face declares tools, the runtime
intercepts the model's tool-call output and dispatches to the matching
`on` handler. The result is fed back to the model in the next turn,
all within a single `think()` call.

```soma
cell agent Researcher {
    face {
        signal research(topic: String) -> Map
        tool search(query: String) -> String "Search the web"
    }
    on search(query: String) { http_get("https://api.search.com?q={query}") }
    on research(topic: String) {
        // The LLM may emit multiple search() tool calls; each one
        // dispatches to on search() before the LLM continues.
        think("Research {topic} thoroughly", map("max_tokens", 2000))
    }
}
```

## Configuration

The model is configured in `soma.toml`:

```toml
[agent]
provider = "ollama"          # ollama | openai | anthropic | custom
model = "gemma3:12b"

# OR
provider = "anthropic"
model = "claude-opus-4-7"
key = "sk-ant-..."            # or SOMA_LLM_KEY env var
```

The runtime reads this once at startup. Different cells can use
different models via `[model: name]` cell annotation and a
`[models.name]` table.

## Token budget

`set_budget(N)` declares a hard cap on tokens for the current handler:

```soma
on research(topic: String) {
    set_budget(5000)              // hard cap; subsequent think() throttles
    let facts = think("Research {topic}", map("max_tokens", 2000))
    let summary = think("Summarize: {facts}", map("max_tokens", 500))
    map("facts", facts, "summary", summary, "used", tokens_used())
}
```

`tokens_used()` returns the cumulative count for the handler.

## Why bounded `think` matters

It's the **mechanism** that makes Soma's "verified AI agents" claim
true. Two things compose:

1. **Per-call bound.** `max_tokens: 500` means the result is ≤ 4 × 500
   bytes (conservative). The [[budget-proof]] reads the map literal at
   compile time and emits a closed-form bound.
2. **Think-isolation.** Even if the LLM hallucinates wildly, CTL
   safety properties of the state machine **still hold** when all
   transition targets are literals. The [[think-isolation]] proof in
   Coq makes this formal.

Combined: a cell can call `think()` inside a handler and the compiler
still proves termination, peak memory, and state-machine safety.

## `delegate` — cross-cell calls

For cell-to-cell calls (LLM or pure compute), use `delegate`:

```soma
let result = delegate("Optimizer", "optimize", input_map)
if result.error != () { ... }
```

`delegate` is *not* bounded by default (the target cell's response
size is unknown to the caller). It triggers a budget advisory unless
the target's face contract advertises a bound.

## Memory: `remember` / `recall`

Persistent agent memory across handler calls:

```soma
remember("user_preferences", to_json(prefs))
let prev = recall("user_preferences")
```

Backed by a `__agent_memory` slot the runtime auto-creates. Same
durability story as `[persistent]` memory.

## Human-in-the-loop

```soma
on publish(article: String) {
    approve("publish: {article.title}")    // blocks until human approves
    // ...
}
```

`approve` triggers a UI prompt; the handler pauses until granted.

## Trace

```soma
let log = trace()       // List<Map> of every think, tool call,
                         // transition, delegate this handler
```

Useful for explaining agent behavior. Also feeds [[refinement]]
effect summaries.

## Examples

A minimal agent:

```soma
cell agent Summarizer [model: claude] {
    face { signal summarize(text: String) -> String }
    on summarize(text: String) {
        think("Summarize in 3 bullets: {text}", map("max_tokens", 400))
    }
}
```

A bounded-budget agent with tools:

```soma
cell agent Researcher {
    face {
        signal research(topic: String) -> Map
        tool search(q: String) -> String "Web search"
    }
    state work { initial: idle  idle -> researching -> done  * -> failed }

    on search(q: String) {
        http_get("https://api.search.com?q={q}", map("max_bytes", 65536))
    }
    on research(topic: String) {
        set_budget(8000)
        transition("t", "researching")
        let facts = think("Research: {topic}", map("max_tokens", 3000, "timeout", 60000))
        let summary = think("Synthesize: {facts}", map("max_tokens", 1000))
        transition("t", "done")
        map("summary", summary, "tokens_used", tokens_used(), "trace", trace())
    }
}
```

Run `soma verify` on this cell: termination proven, eventually(done |
failed) proven, peak memory ≤ declared budget.

## Edge cases

- `think()` is **synchronous**. The handler blocks until the model
  responds (or `timeout` fires). No async / await yet.
- If `timeout` fires, the call returns `()` (Unit) — handlers should
  defensively check.
- A handler calling `think()` inside a `[native]` block is rejected by
  [[think-isolation]] checks.
- `think_json` requires the model to return valid JSON or the call
  returns `()`.

## What this does NOT cover

- Streaming responses — see `sse()` in [[stdlib-http]] for output
  streams, but the model itself doesn't stream in V1.5.
- Tool-call ordering across multiple LLM turns — the runtime
  guarantees per-turn sequential dispatch but doesn't expose
  fine-grained control.
