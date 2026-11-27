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

The current release is **Soma 3.4.0** ([release notes](/changelog/3.4.0)). It runs on Linux and macOS. Soma is experimental: [status](/status) says what that means.

<style>
.feat{display:block;margin:1.8rem 0 .4rem;border:1px solid var(--accent);border-radius:12px;padding:1.2rem 1.3rem;background:var(--panel);color:var(--fg)}
.feat:hover{text-decoration:none;box-shadow:0 0 0 1px var(--accent)}
.feat .k{font:600 .75rem var(--mono);letter-spacing:.08em;text-transform:uppercase;color:var(--accent)}
.feat .t{display:block;font-size:1.45rem;line-height:1.2;font-weight:700;letter-spacing:-.015em;margin:.35rem 0 .5rem}
.feat .d{display:block;color:var(--muted);font-size:.98rem;margin:0 0 .9rem}
.feat .n{display:flex;flex-wrap:wrap;gap:.5rem 1.4rem;margin:0 0 .9rem}
.feat .n span{font-size:.85rem;color:var(--muted)}.feat .n b{display:block;font:700 1.25rem var(--mono);color:var(--fg)}
.feat .n .a b{color:var(--accent)}
.feat .go{font-weight:600;color:var(--accent)}
</style>
<a class="feat" href="/orbite"><span class="k">Case study · Orbite</span><span class="t">Who moves? The protocol fits in 1,923 lines of Soma — its safety rules proven on the real code, in two seconds.</span><span class="d">When two satellites are about to collide, exactly one must move. Orbite settles it with rules borrowed from the sea; <code>soma verify</code> proves its 13 safety rules on the real code, bodies included. Around it, twenty AI agents built the 69,000-line institution. Replayed on the 2019 Aeolus–Starlink near-miss, it decides three days before the closest approach — where ESA had to act alone.</span><span class="n"><span class="a"><b>1,923</b>lines: the protocol</span><span class="a"><b>13 / 13</b>rules proven, bodies included</span><span><b>1.8 s</b>to verify</span><span><b>69,150</b>lines around it, by 20 agents</span><span class="a"><b>−69 h</b>decision in the 2019 replay</span></span><span class="go">Read the case study, then download it and check it yourself →</span></a>

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

The guide: [Large programs, skeleton first](/guide/skeleton). Step by step: [lesson 6](/learn/6-a-large-program-skeleton-first). At scale: [Orbite](/orbite), a space traffic protocol whose 1,923-line core is proven whole, and the 69,000-line institution twenty agents built around it.

## Business rules, outside the code

A proof that the code keeps to its structure says nothing about what the structure is for. In one 12,000-line program written by agents, `soma verify` passed from the first day while scenario tests found the same rule missing from one more path, three times in a row. Soma has a place for what the business requires: a **law**, a file of terms and rules in sentences, written by whoever owns the rules. It names no code.

```text file=lois/sanctions.law
law Sanctions

term Holder  "Whoever holds units of the fund, on its register."
term Payment "Cash paid by the fund to a holder: a redemption, an indemnity."

rule "SAN-02" "No payment to a frozen holder"
    about  Payment, Holder
    source "Regulation (EU) 269/2014, art. 2"
    owner  "compliance"
```

```toml file=soma.toml
[law]
files = ["lois/sanctions.law"]
```

The code is aligned on the law. A signal says which term it `realizes`; a refusal says which rule it is `under`. Here two signals pay a holder, and the second one was written later, without the check:

```soma file=app.cell
skeleton Fund {
    memory {
        frozen: Map<String, Bool> [persistent]
        paid: Map<String, Int> [persistent]
    }

    face {
        signal freeze(holder: String) -> Bool
            writes frozen

        // Pays a redemption of `n` to `holder`.
        signal redeem(holder: String, n: Int) -> Int
            realizes Payment
            reads frozen
            writes paid

        // Pays an indemnity after a pricing error.
        signal indemnify(holder: String, n: Int) -> Int
            realizes Payment
            reads frozen
            writes paid
    }
}

flesh Fund {
    on freeze(holder) {
        frozen.set(holder, true)
        return true
    }

    on redeem(holder, n) {
        require (frozen.get(holder) ?? false) == false else Frozen "{holder} is frozen" under "SAN-02"
        paid.set(holder, (paid.get(holder) ?? 0) + n)
        return n
    }

    on indemnify(holder, n) {
        paid.set(holder, (paid.get(holder) ?? 0) + n)
        return n
    }
}

cell test FrozenHolder under "SAN-02" {
    rules {
        assert Fund.freeze("ana")
        assert_fails Fund.redeem("ana", 100) matching "Frozen"
    }
}
```

```run
soma law app.cell --lock
!soma check app.cell
```

The forgotten path is an error, with the rule, its source and the signal. The body is rewritten — one handler, nothing else — and the table says where each rule is served:

```text file=body.txt
require (frozen.get(holder) ?? false) == false else Frozen "{holder} is frozen" under "SAN-02"
paid.set(holder, (paid.get(holder) ?? 0) + n)
return n
```

```run
soma fill app.cell Fund.indemnify --body-file body.txt
soma law app.cell
```

The guide: [The law](/guide/law). What a tag proves and what it does not: [the method](/guide/method).

## A method, held by the toolchain

Three layers, three owners: the **law** (the owner of the rules), the **skeleton** (the architect), the **flesh** (the coding agent). Each is written before the one below, and the compiler holds each to the one above. The agent's loop is three commands, and nothing in it relies on remembering to run a check:

```
soma next app.cell                     # the ONE thing to do now: an error, a rule the law asks, the next hole…
soma skeleton app.cell Cell.signal     # a hole: its brief, one screen
soma fill app.cell Cell.signal         # write that one body; it comes back checked
```

`soma init --agent` puts in a project what an agent is handed — a 3 KB skill, the `AGENTS.md` block, and a hook that runs `soma check` after every edit — and `soma doctor` says when they are no longer the binary's. The person who answers for the program reads the law, the skeleton and the last line of `soma verify`; not the flesh. [The method](/guide/method) is the whole of it, with what each step establishes and what it does not.

## Where to start

<div class="grid">
<div class="card"><h3><a href="/orbite">Orbite: a case study</a></h3><p>Space traffic coordination: a 1,923-line protocol proven whole, and 69,000 lines written by 20 agents around it.</p></div>
<div class="card"><h3><a href="/learn/">Learn Soma in six lessons</a></h3><p>Each lesson is a program, the commands run on it, and what they printed.</p></div>
<div class="card"><h3><a href="/guide/method">The method</a></h3><p>Law, skeleton, flesh: the order of work with a coding agent, and what each step establishes.</p></div>
<div class="card"><h3><a href="/guide/language">The language guide</a></h3><p>The whole core language on one page: what <code>soma docs agent</code> prints.</p></div>
<div class="card"><h3><a href="/reference/">Reference</a> and <a href="/builtins/">builtins</a></h3><p>Every construct, and every builtin with its exact signature.</p></div>
<div class="card"><h3><a href="/programs/">327 verified programs</a></h3><p>Payments, interlocks, medical devices, games, agents: source and the toolchain's real output.</p></div>
<div class="card"><h3><a href="/errors/">Diagnostics</a> and <a href="/gotchas/">pitfalls</a></h3><p>What the compiler says when a program is wrong, and how to fix it.</p></div>
<div class="card"><h3><a href="/agents">For AI agents</a></h3><p>What to fetch, the loop to run, and when not to pick Soma.</p></div>
</div>

## What is proven, what is not

| Proven for every input | Checked at runtime | Not covered |
|---|---|---|
| Every declared state machine: reachability, deadlocks, liveness, temporal properties, and that handlers take only declared edges. An invariant whose every write is bounded. Termination and declared cost bounds. | Every invariant on every write, including one between two slots of a cell. Transitions, guards, typed parameters, token budgets. | Rules between two cells that `soma.toml` does not state; business rules nobody wrote in a [law](/guide/law), and whether a refusal tagged with a rule says what the rule says; effects outside the program (HTTP, files, LLM answers); cluster behaviour. |

The full table: [guarantees](/guide/guarantees). The limits of the implementation: [status](/status).

## The command line

```
soma check app.cell      # static checks; every error names its fix (--json for agents)
soma verify app.cell     # proves state machines, invariants, termination, cost bounds
soma test app.cell       # runs `cell test` assertions
soma run app.cell handler arg1 arg2
soma serve app.cell -p 8080
soma skeleton app.cell   # the program's structure, without bodies
soma next app.cell       # the one thing to do now; soma fill writes one body
soma law app.cell        # each business rule: where the code serves it, its tests
soma doctor              # is this project's agent setup the binary's?
soma explain E0028       # what a diagnostic means
soma docs agent          # the language guide, offline
```

Every command: [command line](/cli/). Source code: [github.com/soma-dev-lang/soma](https://github.com/soma-dev-lang/soma). The paper: [Scale as a type](/paper).
