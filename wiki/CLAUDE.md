---
name: schema
description: Schema and conventions for the Soma LLM wiki. Read this first.
type: schema
---

# Soma Wiki — Schema

This directory is an **LLM-readable wiki** for the Soma language,
following the Karpathy LLM Wiki pattern: structured, interlinked markdown
optimized for LLM consumption rather than human browsing.

Authoritative project sources live elsewhere
(`README.md`, `SOMA_REFERENCE.md`, `SOMA_SPEC.md`, `VISION.md`, `PAPER.md`,
`docs/SEMANTICS.md`, `docs/SOUNDNESS.md`). This wiki **synthesizes** them
into small, focused pages an LLM can ingest one at a time without losing
context.

## Layout

```
wiki/
├── CLAUDE.md           this file — read first
├── INDEX.md            one-line hooks per page
├── concepts/           the mental model: cell, face, memory, state, scale, ...
├── features/           syntactic constructs: sum types, pattern matching, ...
├── verification/       what the compiler proves
├── stdlib/             builtin function reference, grouped by domain
└── synthesis/          cross-cutting: workflow, comparisons, manifesto
```

## Page types

Each page is one of four types, declared in frontmatter:

- **concept** — a single primitive of the language (cell, signal, handler).
  Explains *what it is* and *why it exists*. ~80–150 lines.
- **feature** — a syntactic construct or pattern. Explains *the syntax*
  and *what it desugars to*. Has worked code examples.
- **reference** — a list of builtins, properties, or other enumerable items.
  Tight, scannable, no narrative.
- **synthesis** — cross-cutting essay. Workflow, comparisons, manifesto.

## Frontmatter convention

```yaml
---
name: short-kebab-case-slug          # matches the filename without .md
description: one-line summary used in the INDEX and cross-link tooltips
type: concept | feature | reference | synthesis
since: V1.0 | V1.3 | V1.4 | V1.5     # when the feature first existed
related: [other-page, ...]            # cross-references (slug list)
---
```

## Cross-linking

Use `[[slug]]` for internal links. The slug matches the filename in any
subdirectory (e.g. `[[cell]]` for `wiki/concepts/cell.md`). Resolution is
flat — slugs must be unique across the whole wiki.

External references use ordinary markdown links.

## Writing rules

1. **Lead with the "why".** Each page opens with a 1–3 sentence
   explanation of *what problem this primitive solves*. Then syntax,
   then examples, then edge cases.
2. **No tables.** Prefer multi-level bullet lists. LLMs digest them
   better than tables (the layout-vs-meaning ambiguity confuses
   chunk-aware retrieval).
3. **Define every abbreviation on first use** — including project
   shorthand (`SQ access` = sample-and-query access).
4. **Code blocks must be runnable** — every example should typecheck
   against the current language. If it requires context, name the file.
5. **Headers carry semantics.** `## Examples`, `## Edge cases`,
   `## What this does NOT cover` are reserved subheaders.
6. **Cap each page at ~200 lines.** Longer means split.
7. **No emojis** — they tokenize unpredictably across LLMs.

## Sources used to compile this wiki

- `README.md` — overview, tagline
- `VISION.md` — five bets, the manifesto
- `PAPER.md` — Scale as a Type academic write-up
- `SOMA_REFERENCE.md` — language reference for agents
- `SOMA_SPEC.md` — formal spec
- `AGENT.md` — agent-facing guide
- `SKILL.md` — skill format docs
- `docs/SEMANTICS.md` — operational semantics
- `docs/SOUNDNESS.md` — what's mechanically verified
- `docs/ADVERSARIES.md` — adversary models per property
- `SUM_TYPES_DESIGN.md` — V1.5 sum types RFC
- `MEMORY_DESIGN.md` — integer memory system
- `NATIVE_BIGINT_PLAN.md` — native compilation roadmap
- the source tree under `compiler/src/`

When information conflicts, the source tree wins.

## Update protocol

When a new feature lands:

1. Add the feature page in `features/` with frontmatter `since: V1.X`.
2. Update affected `concepts/` pages if the primitive itself changed.
3. Add one line to `INDEX.md`.
4. Cross-link from at least one synthesis page.

When a feature is deprecated, do not delete — mark `status: deprecated`
in frontmatter and add a `**Deprecated in V1.X. See [[replacement]].**`
note at the top.

## What this wiki does NOT cover

- Build system internals (cargo, rustc flags) — see `CONTRIBUTING.md`.
- The Coq proof internals — see `docs/rigor/`.
- Provider/sidecar interface — see `SOMA_PROVIDER_SPEC.md`.
- Application-specific code in `mft/`, `rebalancer/`, etc. — those are
  *consumers* of the language, not part of it.
