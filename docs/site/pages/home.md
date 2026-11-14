---
title: Soma
full_title: Soma — a programming language for programs written by coding agents
description: Soma is an open-source programming language for programs written by AI coding agents. One .cell file holds data, state machine, handlers, tests and HTTP routes; soma check names the fix for every error and soma verify proves lifecycles and limits.
type: WebPage
h1: no
---

# Soma

<h1>Soma, a language for programs written by coding agents</h1>

<p class="lede">Soma is an open-source programming language (MIT) for services whose correctness is a lifecycle and a few hard limits: approvals, quotas, escrow, agent workflows. One <code>.cell</code> file holds the data, the state machine, the handlers, the tests and the HTTP routes. <code>soma check</code> answers every mistake with the form to write instead; <code>soma verify</code> proves the lifecycle and the limits it can, and says which ones it only checks at runtime.</p>

<div class="install">curl -fsSL https://soma-lang.dev/install.sh | sh</div>

The current release is **Soma 3.2.3** ([release notes](/changelog/3.2.3)). It runs on Linux and macOS. Soma is experimental: [status](/status) says what that means.

## One rule, and the proof that it holds

A quota of 100 units per user. The limit is written once, next to the data, as an `invariant`; the handler states its own preconditions with `require`.

```soma file=quota.cell
cell Quota {
    memory {
        used: Map<String, Int> [persistent]
        invariant used >= 0 && used <= 100
    }

    on consume(user: String, n: Int) {
        require n > 0 else BadAmount
        let before = used.get(user) ?? 0
        require before + n <= 100 else QuotaExceeded
        used.set(user, before + n)
        return 100 - (before + n)
    }
}
```

The verifier proves the invariant for every input, then the program runs. These outputs were printed by the `soma` binary when this page was built:

```run
soma verify quota.cell
soma run quota.cell consume ana 60
!soma run quota.cell consume ana 60
!soma run quota.cell consume ana -5
soma run quota.cell consume ana 40
```

The second call fails and leaves the slot unchanged: a handler that fails leaves no write behind. Remove the `QuotaExceeded` line and `soma verify` no longer proves the bound; it reports it as checked at runtime and names the line that would make it provable. [Lesson 3](/learn/3-a-rule-next-to-the-data-proven) shows it.

## Every error names its fix

Models write habits of other languages. Soma refuses them before anything runs, and the message says what to write:

```soma file=cart.cell
cell Cart {
    memory { items: Map<String, Int> [persistent] }

    on add(sku: String) {
        let current = items.get(sku)
        if current == null {
            items.set(sku, 1)
        }
    }
}
```

```run
!soma check cart.cell
```

Each of the 150 diagnostic codes has a page with a program that triggers it, the real output, and the fix: [Diagnostics](/errors/).

## Large programs, skeleton first

No model holds a 10,000-line program in context. In Soma the structure is written first, as a `skeleton`: memory, invariants, state machines, and for each signal a **frame** saying what its body may read, write, call, and which edges it may take. Bodies come later, in `flesh` blocks, one at a time, each from a one-screen brief (`soma skeleton app.cell Cell.signal`), and `soma check` refuses a body that leaves its frame. `soma verify` proves the lifecycle before any body exists.

```soma file=loans.cell
skeleton Loans {
    memory { holder: Map<String, String> [persistent] }   // loan id -> member

    state loan {
        initial: requested
        requested -> out
        requested -> refused
        out -> returned
    }

    face {
        // Lends a book to `member` under loan `id`.
        signal lend(id: String, member: String) -> String
            writes holder
            moves requested -> out, requested -> refused

        // Closes loan `id`.
        signal close(id: String) -> String
            writes holder
            moves out -> returned
    }
}
```

```run
soma verify loans.cell
soma holes loans.cell
```

The guide: [Large programs, skeleton first](/guide/skeleton). Step by step: [lesson 6](/learn/6-a-large-program-skeleton-first).

## Where to start

<div class="grid">
<div class="card"><h3><a href="/learn/">Learn Soma in six lessons</a></h3><p>Each lesson is a program, the commands run on it, and what they printed.</p></div>
<div class="card"><h3><a href="/guide/language">The language guide</a></h3><p>The whole core language on one page: what <code>soma docs agent</code> prints.</p></div>
<div class="card"><h3><a href="/reference/">Reference</a> and <a href="/builtins/">builtins</a></h3><p>Every construct, and every builtin with its exact signature.</p></div>
<div class="card"><h3><a href="/programs/">327 verified programs</a></h3><p>Payments, interlocks, medical devices, games, agents: source and the toolchain's real output.</p></div>
<div class="card"><h3><a href="/errors/">Diagnostics</a> and <a href="/gotchas/">pitfalls</a></h3><p>What the compiler says when a program is wrong, and how to fix it.</p></div>
<div class="card"><h3><a href="/agents">For AI agents</a></h3><p>What to fetch, the loop to run, and when not to pick Soma.</p></div>
</div>

## What is proven, what is not

| Proven for every input | Checked at runtime | Not covered |
|---|---|---|
| Every declared state machine: reachability, deadlocks, liveness, temporal properties, and that handlers take only declared edges. An invariant whose every write is bounded. Termination and declared cost bounds. | Every invariant on every write, including one between two slots of a cell. Transitions, guards, typed parameters, token budgets. | Rules between two cells that `soma.toml` does not state; business rules nobody wrote; effects outside the program (HTTP, files, LLM answers); cluster behaviour. |

The full table: [guarantees](/guide/guarantees). The limits of the implementation: [status](/status).

## The command line

```
soma check app.cell      # static checks; every error names its fix (--json for agents)
soma verify app.cell     # proves state machines, invariants, termination, cost bounds
soma test app.cell       # runs `cell test` assertions
soma run app.cell handler arg1 arg2
soma serve app.cell -p 8080
soma skeleton app.cell   # the program's structure, without bodies
soma explain E0028       # what a diagnostic means
soma docs agent          # the language guide, offline
```

Every command: [command line](/cli/). Source code: [github.com/soma-dev-lang/soma](https://github.com/soma-dev-lang/soma). The paper: [Scale as a type](/paper).
