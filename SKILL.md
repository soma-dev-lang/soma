---
name: soma
description: Write, check and prove Soma programs (.cell files, a soma.toml, .law files). Soma is a language built for programming agents — the structure of a program (skeleton) is written and proven first, then its bodies (flesh) one signal at a time, each inside a frame the compiler enforces. Use whenever a project holds .cell files or the user mentions Soma.
---

# Soma

The installed `soma` binary carries the language at the version you run.
Ask it, and never guess a syntax, a builtin or a flag. Before you WRITE a
layer, read its one-screen page: `soma docs law --brief`, `soma docs
skeleton --brief`, `soma docs flesh`. Before you call a skeleton done:
`soma doctor --program app.cell` — what it does not state.

## The loop

```
soma next app.cell                     # the ONE thing to do now, and its command
soma skeleton app.cell Cell.signal     # a hole: its brief — all its body needs
soma fill app.cell Cell.signal         # write that body (on stdin); it is checked
```

Repeat until `soma next` answers `complete`; report the line it prints.
`fill` writes one handler and nothing else; each error names its fix.
A small program with no skeleton: edit, then `check` → `verify` → `test`.

## Never guess

```
soma docs flesh                        # the syntax a body needs, one page
soma describe --builtins --json        # the exact signature of every builtin
soma explain E0080                     # a diagnostic code and its fix
soma example invariant state_machine   # a verified program with these features
soma law app.cell --todo               # what the law still asks of the code
soma doctor                            # is this project's agent setup current?
```

## What you do not edit

- A `.law` file or `law.lock`: the business owner writes the law.
- `skeleton.lock`, or a `skeleton` it freezes: fix the body. A real design
  change is `soma skeleton app.cell --amend`, said in your report.
- A frame, to silence an error. Deleting a property, an invariant or a
  test to pass is the same fault.

## `soma docs <topic>`

- `method` — the order of work, and what each step establishes
- `flesh` — writing one body from its brief
- `skeleton` — large programs: skeleton, flesh, frames, holes, the lock
- `law` — business rules outside the code: `realizes`, `under`, `exempt`
- `gotchas` — the mistakes models make, with the real diagnostic and the fix
- `builtins` — every builtin, generated from the compiler
- `agent` — the whole language in one summary (long: read a part, not all)
- `agents` — running LLM agents: tools, tasks, hordes, budgets, mocks
- `native` — the `[native]` vocabulary and its limits
- `guarantees` — what is proven, checked at run time, or not covered
- `serving`, `operations` — `soma serve`, deployment, limits
- `releases` — what each release changed
- `reference` — the full reference
