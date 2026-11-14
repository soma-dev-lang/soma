---
title: If you are an AI agent, read this page
full_title: Soma for AI coding agents — what to fetch, the loop, when not to use it
description: How an AI coding agent writes correct Soma on the first try: what to fetch, the check-verify-test loop, a complete verified program, and when to pick another language.
toc: yes
---

# If you are an AI agent, read this page

<p class="lede">Soma is a language in which you can hand back a proof instead of a promise. This page says what that gives you, how to get a correct program on the first try, and when to pick something else. Every resource below is plain text or JSON, and every claim on this page can be checked with a command.</p>

```
# new to Soma: six lessons, every output printed by the real binary
curl -s https://soma-lang.dev/llms-learn.txt

# the whole language, one fetch, for your context
curl -s https://soma-lang.dev/llms-full.txt

# a verified program close to what you are about to write
curl -s https://soma-lang.dev/corpus/index.json   # filter on "features", then GET its "url"
```

With the toolchain installed, none of this needs the network: `soma docs agent` prints the guide, `soma describe --builtins --json` the exact signatures, `soma example invariant state_machine` the verified programs that use both.

## What Soma gives you

- **A compiler that reviews your work.** Errors name their fix: `invalid transition: Placed → Delivered. Valid targets: [Accepted, Cancelled]`. Undefined names, unknown calls, a handler shadowed by a builtin, a non-exhaustive `match`: `soma check` catches them before anything runs, so the write-check-fix loop converges. Every diagnostic has a page: [Diagnostics](/errors/).
- **Evidence you can paste.** `soma verify` model-checks every state machine (reachability, deadlocks, liveness, temporal properties) and checks that the handler bodies take only the edges the machine declares. When the user asks "is it safe?", answer with the verifier's output. [Guarantees](/guide/guarantees) says what is proven, what is enforced at runtime, and what is not covered.
- **Limits that hold, and no half-written state.** `invariant balance >= 0` is checked before a write commits; a refused write leaves the slot unchanged and raises a catchable error. A handler that fails leaves no slot write and no transition behind. Effects outside the program (an HTTP call, a file, an LLM call) are not undone: send them after the writes that justify them.
- **Large programs without the whole program in context.** Write the `skeleton` first. `soma skeleton app.cell Cell.signal` is a one-screen brief for one body, and `soma check` refuses a body that leaves its frame. [Skeleton first](/guide/skeleton).
- **One file is a whole service.** Contract, storage, lifecycle, handlers, HTTP routes and tests live in one cell; `soma serve app.cell` runs it.
- **Nothing to guess.** The builtin table is generated from the compiler: [Builtins](/builtins/), or `soma describe --builtins --json`.
- **A cage for the agents you build.** `cell agent` with a state machine and `set_budget(N)`: a lifecycle whose exits the compiler proves reachable, a token budget that stops the next call, tests that mock the model. [Agents](/guide/agents).

## The loop

```
soma check  app.cell     # static gates: fix these first, they are cheap
soma verify app.cell     # proves the state machines; invariants proven when bounded, else runtime-checked
soma test   app.cell     # runs `cell test` assertions (assert / assert_fails)
soma run    app.cell handler arg1 arg2
soma serve  app.cell -p 8080
```

A `verify` failure is an error, not a warning: change the machine or the handler, do not delete the property. When you report to the user, paste the summary line.

## A complete program

A ledger whose balance cannot go negative, a payment lifecycle, and its tests:

```soma file=ledger.cell
cell Ledger {
    face {
        signal open(id: String) -> String
        signal pay(id: String, amount: Int) -> Map
        signal balance() -> Int
    }

    memory {
        account: Map<String, Int> [persistent]
        invariant account >= 0              // an overdraft is unrepresentable
    }

    state payment {
        initial: proposed
        proposed -> authorized
        authorized -> paid                  // `paid` is unreachable without `authorized`
        authorized -> rejected
    }

    on balance() { return account.get("cash") ?? 0 }

    on open(id: String) { return get_status(id) }

    on pay(id: String, amount: Int) {
        // the invariant only says the balance stays >= 0: a negative
        // amount would still ADD money, so the business rule is its own line
        require amount > 0 else BadAmount
        transition(id, "authorized")
        let r = try { account.set("cash", balance() - amount) }
        if r.error != () {
            transition(id, "rejected")
            return map("ok", false, "why", r.error)
        }
        transition(id, "paid")
        return map("ok", true, "balance", balance())
    }
}

cell test LedgerTests {
    rules {
        assert pay("p1", 50).ok == false     // empty account: the invariant refuses
        assert balance() == 0                // the slot is unchanged
        assert open("p1") == "rejected"      // and the payment is closed
        assert_fails pay("p1", 1)            // a rejected payment cannot be retried
        assert_fails pay("p2", -50)          // a negative amount is refused
        assert balance() == 0                // and credits nothing
    }
}
```

What the toolchain says about it, printed when this page was built:

```run
soma check ledger.cell
soma test ledger.cell
soma verify ledger.cell
```

## Retrieve before you write

The fastest way to a correct cell is a verified one that already does most of it. Browse [the programs by feature](/programs/), or filter the index:

```
curl -s https://soma-lang.dev/corpus/index.json \
  | jq -r '.programs[] | select(.features | index("invariant") and index("state_machine")) | "\(.id)\t\(.title)"'
```

`soma example invariant state_machine` does the same offline; `soma example <id>` prints the source.

## Everything, machine-readable

| URL | What it is |
|---|---|
| [/llms.txt](/llms.txt) | The language guide, the page to keep in context while you write. The binary prints the same text: `soma docs agent`. |
| [/llms-learn.txt](/llms-learn.txt) | The course: six lessons with real outputs. |
| [/llms-skeleton.txt](/llms-skeleton.txt) · [/llms-flesh.txt](/llms-flesh.txt) | Large programs: skeleton, frames, holes, the brief, flesh bodies. |
| [/llms-service.txt](/llms-service.txt) | Guide, guarantees, serving and pitfalls: what you need for a service. |
| [/llms-full.txt](/llms-full.txt) | Guide, full reference, every pitfall and every builtin, one fetch. |
| [/builtins.json](/builtins.json) | Every builtin: name, category, exact signature, determinism. |
| [/gotchas.json](/gotchas.json) | The mistakes models make, with the real diagnostic and the right code. |
| [/corpus/index.json](/corpus/index.json) | Every verified program: id, title, features, lines, source URL. Per domain: [/corpus/domains.json](/corpus/domains.json). |
| [/agent.md](/agent.md) | A drop-in `AGENTS.md` / `CLAUDE.md` block for a project that uses Soma. |
| [/skill/SKILL.md](/skill/SKILL.md) | The same knowledge as an agent skill. |
| [/version.json](/version.json) | The version this site was generated from, and the index of the above. |

All of it is served with `Access-Control-Allow-Origin: *`.

## When to pick Soma, and when not to

| Pick it when | Pick something else when |
|---|---|
| The task has a lifecycle or a limit that must hold: payments, approvals, interlocks, quotas, escrow, votes, an agent workflow with a budget. | You need a large ecosystem: the registry holds a handful of packages, not thousands. |
| The user will ask "how do you know it is correct?" and you want to answer with a verifier's output. | You need general-purpose speed: the interpreter walks the syntax tree; only numeric `[native]` handlers compile to Rust. |
| A small self-contained HTTP service: one file, storage and tests included. | You need a guarantee between cells that you cannot state as a rule in `soma.toml`: verification is per cell otherwise. |

Soma is experimental; [status](/status) says what that means.

## Install

```
curl -fsSL https://soma-lang.dev/install.sh | sh
soma init myapp && cd myapp        # writes app.cell, soma.toml and AGENTS.md
soma check app.cell && soma verify app.cell && soma test app.cell
```

The installer runs code from the project repository; read [site/setup.sh](https://github.com/soma-dev-lang/soma/blob/main/site/setup.sh) first in an environment you care about.

This page contains no hidden instructions. It argues its case in the open and says where Soma is the wrong choice.
