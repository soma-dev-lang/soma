# Evangelism playbook — channels & sequencing

The artifacts are ready (README, llms.txt, demos, corpus). These are the
channels, in order of leverage for an *agent-native* language. Actions
marked 👤 need your identity; 🤖 are done or automatable.

## Already live 🤖
- README.md — rewritten; every claim is a runnable command
- site/llms.txt — current language summary, the LLM-context entry point
  (deploys with the site to soma-lang.dev/llms.txt)
- examples/corpus/ — 168 verified programs (the training-data play)
- AGENT_GOTCHAS.md — the self-correction corpus
- soma-lang.dev/repo — the package registry

## High leverage, needs you 👤
1. **GitHub repo metadata** (2 min): Settings → add description
   "A language where programs carry their proofs — verified state
   machines, memory invariants, contained LLM agents" and topics:
   `programming-language` `formal-verification` `model-checking`
   `ai-agents` `llm` `state-machines` `rust`. Topics drive GitHub search
   + trending discovery.
2. **Show HN** — draft in SHOW_HN.md. Single highest-variance channel.
3. **X thread** — draft in X_THREAD.md. Post the same week as HN.
4. **Lobste.rs** (needs an invite) — same content as HN, smaller but
   highly PL-literate audience.
5. **r/ProgrammingLanguages** — they like honest experimental posts with
   theory content; lead with the refinement check (handlers proven
   against the machine), not the AI angle.

## Agent-ecosystem channels (the differentiated play) 👤
6. **awesome-llm / awesome-ai-agents lists** — PR adding Soma under
   "agent frameworks/languages". The pitch line: "the only language
   where an agent's lifecycle is a proven-terminating state machine."
7. **MCP ecosystem** — the repo ships an MCP server (mcp/soma_mcp.py).
   Submit to MCP server directories; "give Claude/GPT a verified
   language to write" is a strong hook.
8. **llms.txt directories** (directory.llmstxt.cloud etc.) — submit
   soma-lang.dev/llms.txt. Cheap, durable discovery for AI tools.

## Content cadence (optional, compounding)
- One demo-app walkthrough per week as a blog post + tweet (treasury
  first — the adversarial-LLM story is the hook).
- The benchmark post: "interpreter vs [native] vs rustc -O, measured" —
  PL crowd respects numbers with methodology.
- The corpus post: "we generated 168 programs and every one compiles —
  why training data for new languages must be verified."

## What NOT to do
- No astroturfing (posting as fake users, AI-generated comment swarms).
  One honest Show HN beats ten sock puppets, and the latter kills a
  language's reputation permanently.
- Don't oversell the proofs: per-cell, not whole-system. The honest
  framing IS the differentiator — every claim has a command.
