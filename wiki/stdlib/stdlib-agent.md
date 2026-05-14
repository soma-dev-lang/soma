---
name: stdlib-agent
description: Agent builtins — think, delegate, remember, recall, approve, trace.
type: reference
since: V1.0
related: [think, handler, budget-proof, think-isolation]
---

# Stdlib: agent

Builtins available in `cell agent` cells (and inherited by regular
`cell`s for `delegate`, `remember`, etc.). See [[think]] for the full
mechanism.

## LLM calls

```soma
let answer = think(prompt)                                  // unbounded
let answer = think(prompt, map("max_tokens", 500, "timeout", 10000))  // bounded
let data = think_json(prompt, map("max_tokens", 800))       // returns Map
```

`think` returns a String. `think_json` parses the LLM output as JSON
and returns a Map (or `()` if invalid JSON).

Without bounds in the options map, [[budget-proof]] downgrades the
cell to advisory.

## Cross-cell

```soma
let result = delegate("OtherCell", "signal_name", arg1, arg2)
// Returns the called handler's return value, or { error: "..." } on failure.
```

`delegate` is **synchronous request-response**. The target cell must
be a sibling (same parent), the current cell itself, or an interior
child.

```soma
let r = delegate("Validator", "check", input)
if r.error != () { return r }
```

## Token budget

```soma
set_budget(5000)               // hard cap for this handler
let tokens = tokens_used()      // cumulative usage so far
```

After `set_budget(N)`, subsequent `think()` calls that would exceed N
are throttled or fail. This is enforced at runtime, not statically.

## Persistent agent memory

```soma
remember("user_preferences", to_json(prefs))
let prev = recall("user_preferences")
let similar = recall_similar("user preferences for trading")  // semantic recall
```

Backed by an auto-created `__agent_memory` slot. Same durability story
as a `[persistent]` Map.

`recall_similar` performs vector-similarity search (when the agent's
provider supports embeddings); falls back to substring match
otherwise.

## Human-in-the-loop

```soma
approve("publish article: {article.title}")  // blocks until human approves
```

`approve` triggers a UI prompt (via the running server's dashboard).
The handler pauses until the operator clicks approve or reject. On
reject, the handler returns `()`.

## Trace

```soma
let log = trace()                            // List<Map> of recent events
```

Returns the structured trace of the current handler: every `think`
call, every tool call, every `transition`, every `delegate`. Used for
explaining agent behavior and for [[refinement]] effect summaries.

## Tool dispatch (automatic)

When a `cell agent` declares tools in its face:

```soma
cell agent Researcher {
    face {
        signal research(topic: String) -> Map
        tool search(q: String) -> String "Search the web"
    }
    on search(q: String) { ... }
    on research(topic: String) {
        think("Research {topic}", map("max_tokens", 2000))
        // The LLM's tool-call output is dispatched to `on search`
        // automatically by the runtime.
    }
}
```

The tool-call dispatch is invisible at the source level — `think()`
appears to be a single call, but internally the runtime may iterate
through several model turns interleaved with tool invocations.

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

A multi-step agent with tools and budget:

```soma
cell agent Analyst {
    face {
        signal analyze(report: String) -> Map
        tool fetch(url: String) -> String "Fetch a URL"
        tool calc(expr: String) -> Float "Evaluate math"
    }

    on fetch(url: String) {
        http_get(url, map("max_bytes", 100000, "timeout", 30000))
    }
    on calc(expr: String) { ... }

    on analyze(report: String) {
        set_budget(10000)
        let facts = think("Extract key facts from: {report}",
                          map("max_tokens", 3000, "timeout", 60000))
        let analysis = think("Analyze: {facts}",
                              map("max_tokens", 2000))
        map(
            "analysis", analysis,
            "tokens_used", tokens_used(),
            "trace", trace()
        )
    }
}
```

## What the verifier proves about agent cells

Three things, given the agent uses literal transition targets:

1. **State-machine safety** — CTL properties hold regardless of LLM
   output. See [[think-isolation]].
2. **Memory budget** — peak consumption is bounded when `think` is
   called with `max_tokens`. See [[budget-proof]].
3. **Refinement** — handler effects (transitions, delegations) are
   summarised statically. See [[refinement]].

The LLM is treated as an adversary. The proofs survive.

## Edge cases

- `think()` is **synchronous**. No async. The handler blocks.
- A `think()` with no `max_tokens` makes the cell budget advisory,
  not proven.
- Tool dispatch is automatic; you don't `delegate` to your own tools.
- `recall_similar` requires the provider to support embeddings.
  Ollama, OpenAI, Anthropic all do; check `[agent]` config.
- `approve` is a blocking call. Don't use it inside a `every Ns { }`
  block — it will stall the schedule.

## Related

- [[think]] — full primer on `think()`.
- [[think-isolation]] — the safety theorem.
- [[budget-proof]] — bounding agent memory.
- [[face]] — how tools are declared.
