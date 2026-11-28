---
title: Soma
full_title: Soma — build programs that work, with coding agents
description: Soma helps you build programs that work. It is an open-source programming language for programs written by AI coding agents — your agent writes the code, Soma checks it against what you decided, before it runs. Try it in two minutes.
type: WebPage
h1: no
---

# Soma

<style>
.page.single main{max-width:900px;margin:0 auto}
.hero{padding:2.2rem 0 .6rem}
.hero h1{font-size:clamp(2.3rem,5.2vw,3.4rem);line-height:1.05;letter-spacing:-.035em;margin:0 0 1.1rem;text-wrap:balance}
.hero .lede{font-size:1.3rem;line-height:1.5;color:var(--fg);margin:0 0 .5rem;max-width:40rem}
.hero .sub{color:var(--muted);margin:0 0 1.5rem}
.hero .install{font-size:1.02rem;padding:.9rem 1.1rem;border-color:var(--accent);max-width:34rem}
.hero .cta{display:flex;flex-wrap:wrap;gap:.6rem 1.4rem;margin:1rem 0 0;font-weight:600}
.four{display:grid;grid-template-columns:repeat(2,1fr);gap:1rem;margin:2.2rem 0 0}
.four .card{padding:1.15rem 1.25rem}.four .card h3{font-size:1.12rem}.four .card p{font-size:.98rem}
main h2{font-size:1.75rem;letter-spacing:-.02em;margin:3.6rem 0 .8rem;padding-top:2.4rem;border-top:1px solid var(--line)}
main pre{max-height:25rem;overflow:auto}
.deep{margin-top:4.5rem;border-top:3px solid var(--accent);padding-top:.4rem}
.deep .k{font:600 .78rem var(--mono);letter-spacing:.08em;color:var(--accent)}
.deep + h2{border-top:0;margin-top:.6rem;padding-top:.4rem}
@media (max-width:640px){.four{grid-template-columns:1fr}.hero{padding-top:1rem}}
</style>
<div class="hero">
<h1>Soma helps you build programs that work.</h1>
<p class="lede">Your coding agent writes the code. Soma checks it against what you decided, before it runs.</p>
<p class="sub">A language for programs written by coding agents. Free, open source, one binary, Linux and macOS.</p>
<div class="install">curl -fsSL https://soma-lang.dev/install.sh | sh</div>
<p class="cta"><a href="#try-it-in-two-minutes">Try it in two minutes ↓</a> <a href="/orbite">See a 69,000-line program built with it</a></p>
</div>

<div class="four">
<div class="card"><h3>Mistakes get caught early</h3><p>Every error says what to write instead. Your agent fixes it and moves on, before anything runs.</p></div>
<div class="card"><h3>Your rules stay in force</h3><p>Write a business rule once. Every path that should follow it does, or Soma refuses the program.</p></div>
<div class="card"><h3>You read a page, not the codebase</h3><p>You review the rules and the structure: a few hundred lines. Soma holds the rest of the code to them.</p></div>
<div class="card"><h3>It scales</h3><p>The agent works on one small piece at a time. The same method built a 69,000-line program.</p></div>
</div>

## Try it in two minutes

```
soma init --skeleton demo && cd demo
```

```run setup
soma init --skeleton
```

That gives you a small working project: a counter that cannot go negative, with one piece left to write. Ask what to do next. (Everything below was printed by `soma` when this page was built.)

```run
!soma next app.cell
```

`soma next` names the one thing to do. Ask for its brief: one screen, with everything that piece needs and nothing else.

```run
soma skeleton app.cell Counter.reset
```

Write it, hand it in. Soma checks it on the spot:

```text file=body.txt
counts.set("n", 0)
return 0
```

```run
soma fill app.cell Counter.reset --body-file body.txt
soma next app.cell
soma run app.cell add 5
```

Done. That loop — `next`, the brief, `fill` — is the same on [a 69,000-line program](/orbite). The agent never needs the rest of the code in front of it, and it cannot touch what it was not asked to touch.

**Now hand it to your agent.** The project is ready for one: `soma init` wrote an `AGENTS.md` block, a small skill, and a hook that checks every edit and hands the errors back without being asked. Open the folder in Claude Code (the hook is its own; any agent that reads `AGENTS.md` gets the block) and ask for a feature. Then read `soma skeleton app.cell` — the structure, not the code.

The current release is **Soma 4.0.1** ([release notes](/changelog/4.0.1)). Soma is experimental, and says where: [what is proven and what is not](#what-is-proven-what-is-not), [status](/status).

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

<a class="feat" href="/field-report"><span class="k">Field report · by the agent · in progress</span><span class="t">“The compiler said there was no error. A rule-by-rule rereading found 116.”</span><span class="d">An AI agent is rewriting Kill Bill, an open-source billing system, in Soma: a law of 419 rules, a skeleton of 230 signals, no body yet. Its own account — what the language did for it, what it did not tell it, and the mistakes it made.</span><span class="n"><span class="a"><b>419</b>rules in the law</span><span class="a"><b>84</b>refusals written by no one</span><span class="a"><b>69</b>properties proven before any code</span><span><b>116</b>gaps the compiler did not see</span></span><span class="go">Read the agent's account, mistakes included →</span></a>

## Every error says how to fix it

Agents bring habits from other languages. Soma stops them before anything runs, and tells the agent what to write instead:

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

Each of the 161 diagnostic codes has a page with a program that triggers it, the real output, and the fix: [Diagnostics](/errors/).

## Set a limit. It holds.

A quota of 100 units per user. You write the limit once, next to the data. No code path can break it.

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

Soma proves the limit holds for every input, then the program runs:

```run
soma verify quota.cell --summary
soma run quota.cell consume ana 60
!soma run quota.cell consume ana 60
soma run quota.cell consume ana 40
```

The second call is refused and changes nothing: a call that fails leaves no half-written data. Remove the `QuotaExceeded` line and `soma verify` says so: the limit is then only checked at run time, and it names the line that would make it provable. [Lesson 3](/learn/3-a-rule-next-to-the-data-proven) shows it.

## Big programs, one small piece at a time

No agent can keep a 10,000-line program in its head. So you write the structure first, as a `skeleton`: the data, its limits, the lifecycle, and for each operation what its code is allowed to touch. The code comes later, one operation at a time, each from a one-screen brief. Code that steps outside what it was allowed is refused. And the lifecycle is proven before a line of code exists.

```run setup
rm soma.toml
```

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
soma verify loans.cell --summary
```

The guide: [Large programs, skeleton first](/guide/skeleton). Step by step: [lesson 6](/learn/6-a-large-program-skeleton-first). At scale: [Orbite](/orbite), a space traffic protocol whose 1,923-line core is proven whole, and the 69,000-line institution twenty agents built around it.

## Business rules nobody can forget

The classic bug: a rule is enforced in one place and forgotten in the next. In one 12,000-line program written by agents, the same rule turned up missing from one more path three times in a row. In Soma the rule is written once, in a **law** — a plain file owned by whoever owns the rule, with no code in it:

```text file=lois/sanctions.law
law Sanctions

entity Holder "Whoever holds units of the fund, on its register."
state  frozen(Holder): Bool "The holder is under an asset freeze."
state  paid_to(Holder): Int "What the fund has paid the holder."

action Payment(h: Holder, n: Int) "Cash paid by the fund to a holder: a redemption, an indemnity."
action Freeze(h: Holder) "The holder is placed under an asset freeze."

rule "SAN-02" "No payment to a frozen holder"
    about    Payment, Holder
    source   "Regulation (EU) 269/2014, art. 2"
    owner    "compliance"
    requires Payment(h, n): not frozen(h)
```

```toml file=soma.toml
[law]
files = ["lois/sanctions.law"]
```

The skeleton connects the program to the law: `maps` says where a fact of the law lives in the data, `realizes` says which operation is which action. The code itself holds **no check**. Here a second way to pay was added later, and nobody said it was a payment:

```soma file=app.cell
skeleton Fund {
    memory {
        status: Map<String, String> [persistent]
        paid: Map<String, Int> [persistent]
        invariant paid >= 0
    }

    maps frozen(h) = (status.get(h) ?? "valid") == "frozen"
    maps paid_to(h) = paid.get(h) ?? 0

    face {
        // Places `holder` under an asset freeze.
        signal freeze(holder: String) -> String
            writes status
            realizes Freeze(holder)

        // Pays a redemption of `n` to `holder`.
        signal redeem(holder: String, n: Int) -> Int
            writes paid
            realizes Payment(holder, n)

        // Pays an indemnity after a pricing error.
        signal indemnify(holder: String, n: Int) -> Int
            writes paid
    }
}

flesh Fund {
    on freeze(holder) {
        status.set(holder, "frozen")
        return "frozen"
    }

    on redeem(holder, n) {
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
        assert Fund.redeem("bob", 100) == 100
        assert Fund.freeze("bob") == "frozen"
        assert_fails Fund.redeem("bob", 100) matching "SAN-02"
    }
}
```

```run
soma law app.cell --lock
!soma check app.cell
```

Soma catches the forgotten path, before its code even matters. Say it is a payment, and the rule applies to it — without a line of code:

```soma
// Pays an indemnity after a pricing error.
signal indemnify(holder: String, n: Int) -> Int
    writes paid
    realizes Payment(holder, n)      // the one line added
```

```run setup
perl -0pi -e 's/(signal indemnify\(holder: String, n: Int\) -> Int\n            writes paid\n)/$1            realizes Payment(holder, n)\n/' app.cell
```

```run
soma check app.cell
soma run app.cell indemnify ana 100
soma run app.cell freeze ana
!soma run app.cell indemnify ana 100
```

The refusal comes from the rule itself. The agent that writes the code cannot forget it and cannot weaken it. The guide: [The law](/guide/law).

## You read a page, not the codebase

Nobody reads 70,000 lines an agent wrote. With Soma you do not have to. You read three short things, and Soma holds the code to them:

| You read | Written by | It is |
|---|---|---|
| the **law** | whoever owns the business rules | the rules, in sentences, with their sources |
| the **skeleton** | the architect | the data, the lifecycles, and what each operation may touch — a few hundred lines for a large program |
| the last line of `soma verify`, and `soma law` | Soma | what is proven, what is checked while running, where each rule is enforced |

**A green check is not a faithful program, and Soma says so.** The last line of `soma check` is not "all good": it lists what was not judged — the rules that stay sentences, a signal nobody calls, a path that reaches data without the rules that guard it. [A field report](/field-report) is why: no error, and 116 rules misrepresented.

The agent works below that line and cannot move it. It writes one operation at a time and nothing else; changing the structure is a separate step, and it prints what changed. [The method](/guide/method) is the whole way of working.

## Where to start

<div class="grid">
<div class="card"><h3><a href="/field-report">A field report, by the agent</a></h3><p>Rewriting a billing system under a law of 419 rules: what Soma did, what it did not say, the agent's mistakes.</p></div>
<div class="card"><h3><a href="/orbite">Orbite: a case study</a></h3><p>Space traffic coordination: a 1,923-line protocol proven whole, and 69,000 lines written by 20 agents around it.</p></div>
<div class="card"><h3><a href="/learn/">Learn Soma in seven lessons</a></h3><p>Each lesson is a program, the commands run on it, and what they printed.</p></div>
<div class="card"><h3><a href="/guide/method">The method</a></h3><p>Law, skeleton, flesh: the order of work with a coding agent, and what each step establishes.</p></div>
<div class="card"><h3><a href="/guide/language">The language guide</a></h3><p>The whole core language on one page: what <code>soma docs agent</code> prints.</p></div>
<div class="card"><h3><a href="/reference/">Reference</a> and <a href="/builtins/">builtins</a></h3><p>Every construct, and every builtin with its exact signature.</p></div>
<div class="card"><h3><a href="/programs/">327 verified programs</a></h3><p>Payments, interlocks, medical devices, games, agents: source and the toolchain's real output.</p></div>
<div class="card"><h3><a href="/errors/">Diagnostics</a> and <a href="/gotchas/">pitfalls</a></h3><p>What the compiler says when a program is wrong, and how to fix it.</p></div>
<div class="card"><h3><a href="/agents">For AI agents</a></h3><p>What to fetch, the loop to run, and when not to pick Soma.</p></div>
</div>

<div class="deep"><span class="k">FOR THOSE WHO WANT THE THEORY</span></div>

## Under the hood: what "works" means here

Everything above rests on one idea, and it is a precise one. A program is written in three layers, and each is a **refinement** of the one above:

- A **skeleton is one possible realization of a law.** The law is an abstract machine: entities, states, actions, and conditions in a closed language (`requires Payment(h, n): not frozen(h)`, `always p: refunded(p) <= captured(p)`). The skeleton gives a correspondence — each state read from the program's memory (`maps`), each action bound to a signal (`realizes`). Every behaviour of the program, seen through that correspondence, must be one the law permits.
- A **flesh is one possible filling of a skeleton.** Each body is checked against its frame: the effects it may have — what it reads, writes, calls, and which transitions it takes.

The layer above constrains the one below and does not determine it. That is why a law can exist before any program, and why the code can be rewritten without anyone rereading the structure.

What the toolchain does with that:

| Object | What Soma does | Kind of result |
|---|---|---|
| a state machine | model-checks it: reachability, deadlocks, liveness, temporal properties; and checks that the bodies take only the declared transitions | a proof, for every input |
| an invariant on memory | proves it when every write is bounded; otherwise checks it before each write commits | a proof, or a check at run time — the output says which |
| a rule with a formal reading | derives its guard from the rule and the correspondence: nobody writes it, so nobody can write a lenient one | a check before every call |
| a rule that says what always holds | derives a monitor; `soma refute` plays a trace of calls against it, `soma attack` searches for one | a refutation, or none found — which proves nothing, and says so |
| a program | `soma fingerprint`: one hash for this law, this skeleton, this code, this binary | a claim anyone can replay |

Soma does not claim a program is correct. It says what is proven, what is checked, and what is only exposed to refutation — and keeps the three apart. The reasoning: [the manifesto](/guide/manifesto), [guarantees](/guide/guarantees), [the method](/guide/method); the earlier question of distribution: [Scale as a type](/paper).

## What is proven, what is not

| Proven for every input | Checked at runtime | Not covered |
|---|---|---|
| Every declared state machine: reachability, deadlocks, liveness, temporal properties, and that handlers take only declared edges. An invariant whose every write is bounded. Termination and declared cost bounds. | Every invariant on every write, including one between two slots of a cell. Transitions, guards, typed parameters, token budgets. A rule of the [law](/guide/law) that has a formal reading: its guard is derived and judged before every call. | Rules between two cells that `soma.toml` does not state; business rules nobody wrote in a law; for a rule that is only a sentence, whether the refusal tagged with it says what it says; a rule over a batch or a history; effects outside the program (HTTP, files, LLM answers); cluster behaviour. |

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
