---
title: Orbite: space traffic coordination, written by agents
full_title: Orbite — a 67,000-line space traffic coordination system written by 20 AI agents in Soma
description: Orbite is a simulated, worldwide protocol for avoiding satellite collisions — catalogue, screening, coordination, manoeuvres, escalation to States, liability. 67,000 lines of Soma written by 20 coding agents in parallel, skeleton first, with 97 rules between state machines proven on its skeleton by soma verify.
type: TechArticle
toc: yes
---

# Orbite: space traffic coordination, written by agents

<p class="lede">When two satellites are about to collide, who moves? Today, operators email and phone each other and decide case by case; many cannot be reached in time, and no protocol is shared worldwide. <strong>Orbite</strong> is that protocol, written as a program: neutral, open, and proven where it matters. It was written in Soma by twenty coding agents working in parallel, each seeing only its own part — <strong>67,000 lines in a day</strong>, with its central safety rules proven by <code>soma verify</code> and the rest held by checked bodies and thousands of assertions.</p>

Orbite is a research program: States, operators and objects are fictional, and nothing connects to a real system. What is real is the engineering question it answers — *can agents build a large, safety-critical system, and can anyone trust it?*

## Why this problem

A collision in orbit is not a local incident. It creates thousands of fragments that threaten everyone at that altitude for decades. Avoiding it needs three things software rarely gives together:

- **one decision**: exactly one of the two objects manoeuvres — two manoeuvres can cancel out or make things worse;
- **deadlines that cannot slip**: an alert must be answered before the time of closest approach, or someone else must take over;
- **a trail**: if it goes wrong, States must be able to see who decided what, on which data, and who was told.

These are properties of a *lifecycle*, across organisations. That is exactly what Soma's state machines, frames and rules between machines are for.

## What Orbite does

<div class="grid">
<div class="card"><h3>Catalogue</h3><p>States, operators, sensor providers; every object in orbit with its owner, State of registry, regime and propulsion; the UN registry; transfers of ownership.</p></div>
<div class="card"><h3>See</h3><p>Ephemerides published by owners, checked on arrival against independent observation; sensor network, orbit determination, unannounced manoeuvres detected.</p></div>
<div class="card"><h3>Screen</h3><p>All-against-all screening of the catalogue in a native kernel; conjunctions opened, levelled 1 to 3, and every manoeuvre plan screened before approval.</p></div>
<div class="card"><h3>Decide</h3><p>A coordination per conjunction: default designation from the rules of the road voted by States, answers, arbitration with four eyes, one lock per decision.</p></div>
<div class="card"><h3>Act</h3><p>Manoeuvre plans that pass a chain of checks, execute in their window, and are verified against observation afterwards.</p></div>
<div class="card"><h3>Escalate</h3><p>An operator who does not answer is escalated: on-call, mission lead, the State's focal point, then Orbite takes over. Silence never blocks.</p></div>
<div class="card"><h3>Re-entries &amp; end of life</h3><p>Re-entry campaigns and risk on the ground, States notified; disposal plans, passivation, active debris removal with consent of owner and State.</p></div>
<div class="card"><h3>Liability</h3><p>Evidence files frozen at the instant of each decision, claims between States, data shared only with whom it may be.</p></div>
</div>

Twenty subsystems in all, from access control and the API gateway to space weather, compliance and operations.

## A real incident, replayed

In September 2019 ESA's Aeolus had to dodge Starlink 44 alone after its messages went unanswered. [The incident replayed in Orbite](/orbite-aeolus), on the real code: the alert, the silent operator escalated up to the States, Orbite taking over, one designation, a screened and verified manoeuvre — and the one thing Orbite would have done worse than ESA.

## Six properties, written down before any code

The project started from six sentences. Everything else serves them.

| # | Property | How Orbite holds it |
|---|---|---|
| 1 | A conjunction has at most one standing designation; never two avoidance manoeuvres for it | a lock per conjunction, taken only for an agreed or arbitrated coordination and lifted only during a reopening; the coordination *decided* only while the lock is held — **proven** (same id on both machines) |
| 2 | An object without propulsion, out of service or in anomaly is never designated | the lock refuses it; any loss of capability reopens the coordinations it was designated in — rule + bodies + tests |
| 3 | A crewed object manoeuvres only if it is the only one able to | checked where the lock is taken — bodies + tests |
| 4 | No manoeuvre is approved without screening its trajectory, on stale data, or for a suspended operator | a chain of states, one condition per step — first step **proven**; the others are rules through a link, open until the bodies writing the links are judged — rules + bodies + tests |
| 5 | Every level-3 alert is answered before its deadline or escalated; a silent operator never blocks | deadlines driven by one clock, escalation, take-over by Orbite — rules + end-to-end test |
| 6 | A decision is traceable to the data it rested on; that data goes only to authorised parties | evidence files, frozen pieces, controlled reads — rules + end-to-end test |

## The proof, in miniature

Property 1 fits on a screen. A `Lock` per conjunction and a `Coordination`, tied by four rules on their edges: the lock is *taken* only for an agreed coordination and *lifted* only while it is being released; the coordination is *decided* only while the lock is held and *reopened* only once it has been lifted. No path leads to a second designation. No body exists yet — this is the skeleton — and `soma verify` already proves it for **every body that will ever be written inside these frames and honour the conditions on their edges**:

```soma file=orbit.cell
// One decision per conjunction. The lock is taken only for an agreed coordination and
// lifted only while that coordination is being released; the coordination is decided
// only while the lock is held, and reopened only once it is lifted.
skeleton Lock {
    memory { holder: Map<String, String> [persistent] }   // conjunction -> object that manoeuvres
    state lock {
        initial: free
        free -> held
        held -> lifted
        lifted -> held
        free -> closed
        held -> closed
        lifted -> closed
    }
    face {
        // Designates `object` for conjunction `c`: only for an agreed coordination.
        signal take(c: String, object: String) -> String
            writes holder
            moves free -> held when Coordination.agreed, lifted -> held when Coordination.agreed
        // Lifts the designation: only while its coordination is being released.
        signal release(c: String) -> String
            writes holder
            moves held -> lifted when Coordination.releasing
        // The conjunction is over: the lock is retired with it, the designation cleared.
        signal retire(c: String) -> String
            writes holder
            moves free -> closed, held -> closed, lifted -> closed
    }
}

skeleton Coordination {
    state coordination {
        initial: open
        open -> agreed
        agreed -> decided
        decided -> releasing
        releasing -> reopened
        reopened -> agreed
        releasing -> closed
        open -> closed
        agreed -> closed
        reopened -> closed
        decided -> closed
    }
    face {
        // Both operators accept the proposed manoeuvre.
        signal agree(c: String) -> String
            moves open -> agreed, reopened -> agreed
        // Takes the decision: only while the lock is held, so only one object ever manoeuvres.
        signal decide(c: String) -> String
            moves agreed -> decided when Lock.held
            calls Lock.take
        // The situation changed (propulsion lost, new data): release, lift the lock, reopen.
        signal reopen(c: String) -> String
            moves decided -> releasing, releasing -> reopened when Lock.lifted
            calls Lock.release
        // The conjunction is over (resolved, passed), decided or not.
        signal close(c: String) -> String
            moves open -> closed, agreed -> closed, reopened -> closed, decided -> closed, releasing -> closed
            calls Lock.retire
    }
}
```

```run
soma verify orbit.cell
```

The five `✓ entering …` lines are the proof, each over every run of the handlers on the product of both machines. A reviewer's first attempt — lift the lock while the decision stands, then take it for the other object — is a run these rules exclude. (The `note: cross-cell` lines that follow are general advice the tool prints whenever a signal moves its machine and calls another cell; the rule here is stated on the edge, and the `✓` is its proof.)

Now add the shortcut someone always wants, an "urgent" decision that skips the lock:

```soma file=shortcut.cell
skeleton Lock {
    state lock {
        initial: free
        free -> held
        held -> free
        held -> closed
    }
    face {
        signal take(c: String) -> String     moves free -> held
        signal release(c: String) -> String  moves held -> free
        signal retire(c: String) -> String   moves held -> closed
    }
}

skeleton Coordination {
    state coordination {
        initial: open
        open -> agreed
        agreed -> decided
        decided -> closed
    }
    face {
        signal agree(c: String) -> String
            moves open -> agreed
        signal decide(c: String) -> String
            moves agreed -> decided when Lock.held
            calls Lock.take
        // An "urgent" shortcut that forgets the lock.
        signal force(c: String) -> String
            moves agreed -> decided
        signal close(c: String) -> String
            moves decided -> closed
            calls Lock.retire
    }
}
```

```run
soma verify shortcut.cell
!soma verify --strict shortcut.cell
```

The verifier names the run that breaks the rule — `Coordination.agree → Coordination.force` — before a single line of body is written. It is an *open obligation*, not yet a failure: `force` is a hole, and a body could still refuse to take the edge without the lock. A body that takes it fails `verify`; and `--strict`, the mode for CI, fails right away, as the second command shows. In Orbite, these are the rules that tie the real lock (`CoordVerrou`) to the real coordination (`CoordDossier`), in a program of 156 machines.

## The approval chain

Property 4 is a chain of states in the manoeuvre plan's machine: every step can only be taken under one condition held by *another* subsystem. This is the real frame of `ManPlan.approuver` (the code is in French — Orbite was specified in French):

```soma
signal approuver(h: Horodatage, plan: String, par: String) -> ManEtatPlan
    moves crible    -> recoupe   when ConjCriblagePlan.sur,                       // screening still holds
          recoupe   -> coordonne when CoordDossier.arretee  via verrou_de,        // coordination decided
          recoupe   -> habilite  when OpMembres.actif       via proprietaire_de,  // (not an avoidance)
          coordonne -> habilite  when OpMembres.actif       via proprietaire_de,  // operator active
          habilite  -> localise  when EphSuivi.a_jour       via objet_de,         // ephemeris fresh
          localise  -> capable   when RefObjet.en_service   via objet_de,         // object in service
          capable   -> approuve  when ManEvitement.engage   via verrou_de         // the lock is ours
```

A plan that is not an avoidance (station-keeping, a transfer) has no coordination and goes from `recoupe` straight to `habilite`; an avoidance passes through `coordonne`, and the body sends it there. Every other step is taken by every plan: screening, operator, ephemeris, catalogue, and, at the end, the avoidance lock — which an avoidance plan holds only once its coordination is decided. `via` names the slot that links the plan to the instance of the other machine. Such a rule is proven once `soma verify` has seen, in the body that writes the link, that it is written once, at creation; until then it is an *open obligation*.

## How twenty agents built it

No model holds 67,000 lines. None had to.

1. **The contract (530 lines)** — written first: common types (distances in metres, probabilities in billionths, roles with their scope), the audit journal, and twenty *façades* — the only signals a subsystem may call in another.
2. **Twenty architects, in parallel** — each wrote the skeleton of one subsystem (~1,000 lines: memory, invariants, machines, frames, comments that are the brief of every body), seeing only the contract. Then `soma check` and `soma verify` on the whole.
3. **Contract v2** — the architects' reports, merged: one central clock (the only reader of time, besides the HTTP entry point), ninety new façade signals, nine signatures changed. Every subsystem brought into line, re-proven.
4. **Rules between subsystems** — ten agents, two subsystems each, turned about seventy cross-cutting requirements into rules on edges: *a coordination opens only on a conjunction in alert*, *a collision is transmitted only with a frozen evidence file*…
5. **Bodies** — twenty agents again, each filling every signal of its subsystem from one-screen briefs (`soma skeleton prod.cell Cell.signal`), with unit tests and test doubles for the others.
6. **End to end, without doubles** — a world of three States and two operators, a launch, a conjunction avoided, a silent operator escalated until Orbite arbitrates, a collision and its evidence, a re-entry.

| | |
|---|---|
| Skeleton | 21,177 lines · 220 cells · 156 state machines · 1,787 signals |
| Bodies | 29,997 lines · every signal filled, 0 holes |
| Tests | 15,935 lines (12,125 of tests, 3,810 of test doubles) · 22 suites (one per subsystem, end to end, and the Aeolus replay) · **6,264 assertions, all green** |
| `soma verify`, on the skeleton | **97 rules between machines proven**, 21 of them across subsystems · 149 more through a link (`via`), open until judged on the bodies · 0 failures · 0 unexplored |
| Call graph | no cycle · deepest chain 23 calls |

## What the toolchain caught

The value of a verifier is what it finds before production. On Orbite it found, among others:

- **A cycle across two subsystems** that made the central rules impossible to explore: reopening a coordination cancelled the old plan, and cancelling a plan reopened the coordination. Broken by a distinct gesture — *withdraw at the coordination's request* — and the rules of property 1 were proven on the next run.
- **A trap in five invariants**: a rule between two slots written with `?? 0` refused the very first write — no State could be declared, no operator registered. `soma check` flags the pattern; the agents fixed it.
- **Nine integration gaps** that every unit test missed, because each subsystem tested against doubles: a manoeuvre that could never be judged *conforming*, an escalation that took over at TCA − 6 h — leaving no time to act with a 6-hour notice —, a collision that reopened its coordination before the evidence was frozen, ephemerides refused by the API for a missing `sha256:` prefix…
- **Improvements to the toolchain itself**, found because Orbite is larger than anything it had seen: the success of a `try` is now followed into the branch that tests it; a link key made by the signal itself (`let id = "OR-{k}"`) is accepted when bound once; two handlers of the same name in different cells (`Coordination.close` calling `Lock.close`) are no longer taken for a recursion, while a real cycle `A.f → B.f → A.f` is now named as such; and the reference grammar learnt the `when … via …` rule on an edge. They shipped in [Soma 3.2.4](/changelog/3.2.4), each with a regression test.

## Fast where it counts

The decision layer is interpreted — it is a few lookups per event. The all-against-all screening is not: it is a `[native]` handler, compiled to Rust by `cargo` on first run, looping over position and velocity buffers. One language for the protocol and the numerics, one program to read.

## What is proven, and what is not

Honesty is part of the design:

- **Proven**: the 97 rules above, on the skeleton: they hold for every body that stays in its frame — `soma check` proves that every body does — and takes an edge only under its condition, which `soma verify` judges on the bodies. Every lifecycle reaches a terminal state or returns to its initial one; no machine deadlocks.
- **Tested, not yet proven**: properties 2 to 6 rest partly on bodies (a `require` before a transition) and on 149 rules through a link, checked by 6,264 assertions and an end-to-end scenario.
- **Next**: running `soma verify` on the 1,787 bodies together: it judges that each body honours the conditions on its edges, and closes the 149 open obligations, each proven once the verifier sees, in the body that writes its link, that the link is written once, at creation. That is the work in progress — and the two prover improvements above came from it: many of Orbite's links are keyed by an id the signal makes itself.
- **Simulated**: orbital mechanics are simplified and every actor is fictional. Orbite is a demonstration of method, not a service.
- **Versions**: everything on this page — the two examples and Orbite's figures — was produced with soma 3.2.4, the current release. With 3.2.4, `soma verify app.cell` on the published source proves the 97 rules in about 8 minutes.

## What reviewers found

The page and the code were read by people trying to break them. What they found, and what changed:

- **A second designation, in five lines of test.** The real lock could be lifted while its coordination still stood decided, then taken for the other object. The lock now has a `leve` (lifted) state; it is taken only for a coordination in agreement or arbitration and lifted only during a reopening, both as rules on its edges and as checks in its bodies; the attempt is a regression test. The miniature above was fixed the same way.
- **`--strict` failed on this page's own example** once bodies were written: the verifier took `Coordination.close → Lock.close` for a recursion. Fixed in the verifier (above); the example was renamed meanwhile.
- **Figures**: 1,775 signals was 1,787 (the count `soma holes` prints); 22,107 lines of skeleton counted the façades twice — it is 21,177.
- **Wording**: what is proven was stated more broadly than the proof; the sections above now separate the rules proven on the skeleton, the 149 left open, and what the released verifier reproduces — now all of it, with 3.2.4.

## Download and check it

The whole program is published for anyone — person or agent — who wants to check these claims: 491 files, 67,691 lines of Soma.

- [orbite.tar.gz](/cases/orbite/orbite.tar.gz) — everything, 1.1 MB: `curl -fsSL https://soma-lang.dev/cases/orbite/orbite.tar.gz | tar xz`
- [README](/cases/orbite/README.md) — layout, the commands to run, and what each one prints
- [index.json](/cases/orbite/index.json) and [MANIFEST.txt](/cases/orbite/MANIFEST.txt) — every file with its kind, line count, sha256 and URL, to fetch them one by one

With soma 3.2.4, `soma check prod.cell` passes, the 22 test suites pass (6,264 assertions), and `soma verify app.cell` proves the 97 rules (about 8 minutes) — VERIFY OK, 0 failures, 0 unexplored, 149 open obligations. With 3.2.3, the verifier stopped at "more than 12 machines" on these rules; 3.2.4 is the release that changed that. The README says exactly what you will see.

## Try the method

The same path works at any size:

- [Large programs, skeleton first](/guide/skeleton) — skeletons, frames, holes, rules on edges.
- [Lesson 6](/learn/6-a-large-program-skeleton-first) — a large program, step by step.
- [For AI agents](/agents) — what to fetch and the loop to run.
