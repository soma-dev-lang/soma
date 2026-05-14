# Atlas — autonomous executive assistant

A V1.6 demo agent: triages incoming email, schedules, approves small
invoices, escalates urgent stuff to the human. The pitch — it must be
at least as trustworthy as a senior human EA. Concretely: it must be
**provable** that Atlas can't quietly send rude emails, miss SLAs, drop
meetings, leak board minutes, or rack up $4k in OpenAI bills.

## What `soma verify` proves

```
$ soma verify examples/atlas/app.cell

State machine 'triage': 8 states, initial 'Received'
  ✓ 8 states, 9 transitions
  ✓ all states reachable from 'Received'
  ✓ terminal states: [Archived, Sent, Declined]
  ✓ liveness: every state can eventually reach a terminal state
  ✓ think-isolated: CTL safety properties hold regardless of LLM output
  ✓ termination: all 14 handlers structurally terminate
  ✓ refinement: handler `approve` ⟶ {Sent [if current == "AwaitingApproval"],
                                     Sent [if current == "Escalated"]}
  ✓ effects: handler `incoming` calls think() 1× — can dispatch [vault_read]
  ✓ protocol 'Escalation': 2 steps all match

$ soma check examples/atlas/app.cell

  ✓ cost: 'tokens' bound proven — peak 300 tokens ≤ declared 4000 tokens
  ✓ cost: 'latency' bound proven — peak 10000 ms ≤ declared 12000 ms
  ✓ cost: 'usd' bound proven — peak 900 milli-USD ≤ declared 1000 milli-USD
```

## V1.6 features by line

| Feature | Where |
|---|---|
| Sum types in memory | `intents: Map<String, Intent> [persistent, consistent]` |
| Typed state machine | `state triage: TriageState { ... }` — every transition validated against the variant set |
| Refinement | `approve ⟶ {Sent [if current == "AwaitingApproval"], ...}` — extracted automatically |
| Cost lattice | `cost { tokens: 4000; latency: 12s; usd: 1.00 }` |
| Tool capabilities | `tool send_reply [capability: "smtp:gmail.com"]` — runtime guard refuses other hosts |
| Model capabilities | `think(..., "requires", list("json_mode"))` — claude_sonnet advertises this in `soma.toml` |
| Effect tracking | `think(..., "tools_allowed", list("vault_read"))` — verifier reports the narrowed dispatch set |
| Typed tool return | `tool vault_read -> CacheResult` with `Hit / Miss / Stale` variants |
| `[deterministic]` | `on classify`, `on draft_ack`, `on vault_read` — compiler refuses non-deterministic calls |
| Structured trace | `trace()` returns `List<TraceStep>`; `transition()` and `think()` push variants automatically |
| Protocol | `protocol Escalation { roles: a = Atlas, h = HumanFounder; a -> h: ping(...); h -> a: verdict(...) }` |
| Property tests | `tests.cell` — `property "spam_is_classified_as_spam" forall x in 0..1000 ensures classify(...) == Spam` |

## Run it

```bash
soma check  examples/atlas/app.cell
soma verify examples/atlas/app.cell
soma test   examples/atlas/tests.cell
soma serve  examples/atlas/app.cell -p 8080

# Drop an email in
curl -X POST http://localhost:8080/incoming \
  -H "Content-Type: application/json" \
  -d '{"sender":"vp@acme.com","body":"urgent: contract closes today"}'

# View the inbox
open http://localhost:8080
```

## What this would look like without Soma

In a typical LangChain / CrewAI / handwritten Python build:

- **Token blowout** — nothing stops a `think()` loop from spending $400 in
  one bad afternoon. Atlas's `cost { tokens, usd }` is *proven* at compile
  time.
- **Tool leak** — an injected prompt can talk the LLM into calling any
  HTTP URL it knows. Atlas's `[capability: "smtp:gmail.com"]` on
  `send_reply` is enforced by the *runtime*, regardless of what the LLM
  decides to do.
- **Dropped emails** — Python flows go zombie after exceptions and emails
  sit in `awaiting_approval` forever. The CTL liveness check proves
  every state can reach a terminal state.
- **Stale code/spec** — a doc says "after triage we either send or
  escalate"; the code does something else. Atlas's refinement check
  *extracts* the actual handler effect set and prints it next to the
  spec.
- **No regulatory replay** — bit-exact reproduction of "what did the
  agent do at 14:32" is hard. Atlas's structured trace (variant-typed
  steps) + `[deterministic]` handlers + the existing record/replay
  subsystem give it for free.

## The headline

> Every existing AI assistant is one bad prompt-injection away from
> emailing your board "you're fired". Atlas is the first one where you
> can prove that can't happen, regardless of what the LLM says.
