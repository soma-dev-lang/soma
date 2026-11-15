---
title: Orbite: who moves?
full_title: Orbite — twenty AI agents wrote a space traffic protocol in Soma, and the verifier checked it
description: When two satellites are about to collide, who moves? Orbite is a 67,758-line protocol written in Soma by twenty coding agents in a day, with its central rules proven by soma verify. Replayed on the 2019 Aeolus–Starlink near-miss, it settles the decision three days before the closest approach. Download it and check it yourself in eight minutes.
type: TechArticle
h1: no
---

# Orbite

<style>
.orb-kicker{font:600 .8rem var(--mono);letter-spacing:.08em;text-transform:uppercase;color:var(--accent);margin:.6rem 0 .2rem}
.orb-h1{font-size:2.6rem;line-height:1.08;letter-spacing:-.03em;margin:.2rem 0 1rem}
.orb-stats{display:grid;grid-template-columns:repeat(auto-fit,minmax(130px,1fr));gap:.6rem;margin:1.6rem 0}
.orb-stat{border:1px solid var(--line);border-radius:10px;padding:.7rem .8rem;background:var(--panel)}
.orb-stat b{display:block;font:700 1.5rem var(--mono);letter-spacing:-.02em;color:var(--fg)}
.orb-stat span{font-size:.82rem;color:var(--muted);line-height:1.3;display:block}
.orb-stat.hl{border-color:var(--accent)}.orb-stat.hl b{color:var(--accent)}
.orb-cta{display:flex;flex-wrap:wrap;gap:.6rem;margin:1.2rem 0 .4rem}
.orb-cta a{display:inline-block;padding:.55rem 1rem;border-radius:8px;border:1px solid var(--accent);font-weight:600;font-size:.95rem}
.orb-cta a.main{background:var(--accent);color:var(--bg)}
.orb-cta a:hover{text-decoration:none;filter:brightness(1.08)}
.orb-tl{width:100%;height:auto;margin:1rem 0 .4rem;font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",Inter,Roboto,sans-serif}
.orb-steps{display:grid;grid-template-columns:repeat(auto-fit,minmax(190px,1fr));gap:.8rem;margin:1.2rem 0;counter-reset:s}
.orb-step{border:1px solid var(--line);border-radius:10px;padding:.9rem 1rem;background:var(--panel);position:relative}
.orb-step::before{counter-increment:s;content:counter(s);position:absolute;top:.7rem;right:.9rem;font:700 1.4rem var(--mono);color:var(--line)}
.orb-step h3{margin:0 0 .3rem;font-size:1rem}.orb-step p{margin:.25rem 0;font-size:.9rem;color:var(--muted)}
.orb-step .n{font:700 1.1rem var(--mono);color:var(--accent)}
.orb-bug{border-left:3px solid var(--warn);background:var(--panel);border-radius:0 10px 10px 0;padding:.8rem 1rem;margin:0}
.orb-bug h3{margin:0 0 .3rem;font-size:1rem}.orb-bug p{margin:.25rem 0;font-size:.92rem;color:var(--muted)}
.orb-bug .by{font:600 .75rem var(--mono);color:var(--warn);text-transform:uppercase;letter-spacing:.05em}
.orb-bugs{display:grid;grid-template-columns:repeat(auto-fit,minmax(250px,1fr));gap:.8rem;margin:1.2rem 0}
.orb-paths{display:grid;grid-template-columns:repeat(auto-fit,minmax(240px,1fr));gap:.8rem;margin:1.2rem 0}
.orb-path{border:1px solid var(--line);border-radius:10px;padding:1rem 1.1rem}
.orb-path h3{margin:0 0 .4rem;font-size:1rem}.orb-path p{margin:.3rem 0;font-size:.92rem;color:var(--muted)}
.orb-path ul{margin:.4rem 0 0;padding-left:1.1rem;font-size:.92rem}
details{margin:1rem 0}details summary{cursor:pointer;color:var(--accent);font-weight:600;font-size:.95rem}
.orb-note{font-size:.88rem;color:var(--faint)}
@media (max-width:600px){.orb-h1{font-size:1.9rem}.orb-stat b{font-size:1.25rem}}
</style>

<p class="orb-kicker">Case study · space traffic coordination · simulated</p>
<h1 class="orb-h1">Who moves? Twenty AI agents wrote the answer in Soma — and the verifier checked it.</h1>
<p class="lede">When two satellites are about to collide, one of them has to move — exactly one. Today operators settle it by e-mail and phone, case by case; there is no protocol the whole world shares. <strong>Orbite</strong> is that protocol, written as a program: twenty coding agents built it in a day, in parallel, each seeing only its own part. Soma's verifier proves the rules that keep the decision single. Replayed on a real near-miss from 2019, it settles who moves <strong>three days</strong> before the closest approach.</p>
<div class="orb-stats">
<div class="orb-stat"><b>67,758</b><span>lines of Soma, written by agents</span></div>
<div class="orb-stat"><b>20</b><span>agents in parallel, one subsystem each</span></div>
<div class="orb-stat"><b>156</b><span>state machines across 220 cells</span></div>
<div class="orb-stat hl"><b>97</b><span>rules between machines proven by <code>soma verify</code></span></div>
<div class="orb-stat"><b>6,268</b><span>test assertions, all green</span></div>
<div class="orb-stat hl"><b>−69 h</b><span>decision before closest approach in the 2019 replay</span></div>
</div>
<div class="orb-cta">
<a class="main" href="/orbite-aeolus">Replay the 2019 near-miss →</a>
<a href="#check-it-yourself-in-eight-minutes">Download it and check it</a>
<a href="#how-do-you-trust-67-000-lines-no-one-reviewed">How it was built</a>
</div>
<p class="orb-note">Orbite is a research program: States, operators and objects are fictional and nothing connects to a real system. What is real is the question it answers — can agents build a large, safety-critical system that anyone can trust without reading it all?</p>

## September 2019, replayed

On 2 September 2019, ESA's satellite Aeolus burned its thrusters to dodge Starlink 44. The risk had climbed to about 1 in 1,000 — ten times ESA's threshold — and ESA could get no answer from SpaceX: an on-call paging bug, SpaceX said later. No rule said who should move. ESA moved alone, without knowing whether the other satellite would move too.

We replayed that timeline on Orbite's real code, not on a model. The first run was honest and disappointing: Orbite decided *later* than ESA, because it waited for the silent operator until its own take-over deadline. One rule changed — the operator who has to move said yes, the other only stayed silent, so the decision stands — and the replay now looks like this:

<svg class="orb-tl" viewBox="0 0 860 230" role="img" aria-label="Timeline, 28 August to 2 September 2019. In 2019: alert on 28 August, silence, ESA burns alone on 2 September. In Orbite: alert on 28 August, decision settled and plan approved on 30 August at 15:00, burn verified on 2 September.">
<g style="fill:var(--faint);font-size:12px">
<text x="40" y="20">28 Aug</text><text x="180" y="20">29 Aug</text><text x="320" y="20">30 Aug</text><text x="460" y="20">31 Aug</text><text x="600" y="20">1 Sep</text><text x="740" y="20">2 Sep</text>
</g>
<g style="stroke:var(--line);stroke-width:1">
<line x1="40" y1="28" x2="40" y2="215"/><line x1="180" y1="28" x2="180" y2="215"/><line x1="320" y1="28" x2="320" y2="215"/><line x1="460" y1="28" x2="460" y2="215"/><line x1="600" y1="28" x2="600" y2="215"/><line x1="740" y1="28" x2="740" y2="215"/>
</g>
<line x1="810" y1="28" x2="810" y2="215" style="stroke:var(--bad);stroke-width:1.5;stroke-dasharray:4 3"/>
<text x="806" y="226" text-anchor="end" style="fill:var(--bad);font-size:11px;font-weight:600">closest approach</text>
<text x="40" y="58" style="fill:var(--muted);font-size:12px;font-weight:700;letter-spacing:.06em">2019</text>
<line x1="99" y1="80" x2="806" y2="80" style="stroke:var(--line);stroke-width:3"/>
<line x1="402" y1="80" x2="806" y2="80" style="stroke:var(--warn);stroke-width:3;stroke-dasharray:2 5"/>
<circle cx="99" cy="80" r="6" style="fill:var(--faint)"/>
<text x="99" y="104" style="fill:var(--muted);font-size:12px">alert · 1 in 50,000</text>
<circle cx="402" cy="80" r="6" style="fill:var(--warn)"/>
<text x="410" y="70" style="fill:var(--warn);font-size:12px">1 in 1,000 · no answer</text>
<circle cx="806" cy="80" r="7" style="fill:var(--bad)"/>
<text x="796" y="104" text-anchor="end" style="fill:var(--bad);font-size:12px;font-weight:600">ESA burns alone</text>
<text x="40" y="148" style="fill:var(--accent);font-size:12px;font-weight:700;letter-spacing:.06em">ORBITE</text>
<line x1="99" y1="170" x2="806" y2="170" style="stroke:var(--line);stroke-width:3"/>
<line x1="407" y1="170" x2="806" y2="170" style="stroke:var(--accent);stroke-width:5"/>
<circle cx="99" cy="170" r="6" style="fill:var(--faint)"/>
<text x="99" y="194" style="fill:var(--muted);font-size:12px">alert · level 1</text>
<circle cx="407" cy="170" r="8" style="fill:var(--accent)"/>
<text x="398" y="158" text-anchor="end" style="fill:var(--accent);font-size:12px;font-weight:700">30 Aug 15:00 · decided</text>
<text x="415" y="194" style="fill:var(--muted);font-size:12px">plan screened &amp; approved, both told</text>
<circle cx="806" cy="170" r="7" style="fill:var(--ok)"/>
<text x="796" y="158" text-anchor="end" style="fill:var(--ok);font-size:12px;font-weight:600">burn verified</text>
</svg>

| | 2019 | Orbite |
|---|---|---|
| Who moves is decided | never — ESA decides alone | 30 August, 15:00, 69 h before closest approach |
| The other operator knows | no | notified of the decision; its silence escalated up to its State |
| A second manoeuvre is possible | yes | no: one lock per conjunction, a plan for the other satellite is refused |
| The burn is checked | — | trajectory screened before approval, orbit observed after |

[The full replay, minute by minute, with what Orbite still does not do →](/orbite-aeolus)

## One decision, proven

The heart of Orbite is a lock. A conjunction gets one lock; the coordination is *decided* only while the lock is held; the lock is taken only for an agreed coordination and lifted only while it is being reopened. Written as rules on the edges of two state machines, this fits on a screen — and `soma verify` proves it **for every body that will ever be written inside these frames and honour the conditions on their edges**, before a single body exists:

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
soma verify --summary orbit.cell
```

<details><summary>Every line of the proof</summary>

```run
soma verify orbit.cell
```

</details>

The five `✓ entering …` lines in the full output are the rules, each proven over every run of the handlers, on the product of both machines. Now add the shortcut someone always wants — an "urgent" decision, `force`, that takes the edge to `decided` without the lock:

<details><summary>The same two machines, with the shortcut (25 lines)</summary>

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

</details>

```run
!soma verify --strict --summary shortcut.cell
```

The verifier names the run that breaks the rule — `Coordination.agree → Coordination.force` — before any code is written. It is an open obligation while `force` has no body (a body could still refuse the edge); `--strict`, the mode for CI, fails at once. In Orbite the same rules tie the real lock to the real coordination, in a program of 156 machines. A reviewer found a way to designate twice in an earlier version; the fix is the `lifted` state above, and their attempt is now a regression test.

## How do you trust 67,000 lines no one reviewed?

No model holds 67,000 lines in context, and no human read them all. Soma's answer is to write the **structure first**, prove it, and then check each body against the part of the structure it belongs to.

<div class="orb-steps">
<div class="orb-step"><h3>Contract</h3><p class="n">530 lines</p><p>Common types — metres, billionths of probability, roles with their scope — and twenty <em>façades</em>: the only signals a subsystem may call in another.</p></div>
<div class="orb-step"><h3>Skeletons</h3><p class="n">21,189 lines</p><p>Twenty agents in parallel, each writing one subsystem's memory, invariants, state machines and <em>frames</em>: what every signal may read, write, call, and which edges it may take.</p></div>
<div class="orb-step"><h3>Rules, proven</h3><p class="n">97 ✓</p><p>Ten agents turned seventy requirements across subsystems into rules on edges — <em>a coordination opens only on a conjunction in alert</em> — and <code>soma verify</code> proved them.</p></div>
<div class="orb-step"><h3>Bodies, checked</h3><p class="n">30,028 lines</p><p>Twenty agents filled every signal from a one-screen brief. <code>soma check</code> refuses a body that leaves its frame; 6,268 assertions and an end-to-end scenario run them together.</p></div>
</div>

That last point is the one agents need: `soma skeleton prod.cell CoordVerrou.poser` prints everything one body is allowed to touch, and nothing else. An agent writes that body without ever seeing the other 67,000 lines — and cannot quietly reach outside it.

## What the verifier — and the replays — caught

The point of a verifier is what it finds before anyone relies on the system. These are real, from the build of Orbite:

<div class="orb-bugs">
<div class="orb-bug"><p class="by">soma verify</p><h3>A cycle across subsystems</h3><p>Reopening a coordination cancelled the old plan, and cancelling a plan reopened the coordination. The central rules could not even be explored until a distinct gesture broke the loop.</p></div>
<div class="orb-bug"><p class="by">a reviewer</p><h3>Two designations in five lines</h3><p>The lock could be lifted while the decision still stood, then taken for the other satellite. Now a <code>lifted</code> state, two proven rules, and a regression test.</p></div>
<div class="orb-bug"><p class="by">the 2019 replay</p><h3>Escalation 1 → 4 in 30 minutes</h3><p>Every unanswered message — including the escalation's own reminders — climbed one level, waking up two States before anyone had time to answer.</p></div>
<div class="orb-bug"><p class="by">the 2019 replay</p><h3>1,650 breaches in three days</h3><p>A breach notified to a State without a channel created a new breach, in a loop. Unit tests with test doubles could not see it; the real bodies together did.</p></div>
<div class="orb-bug"><p class="by">the 2019 replay</p><h3>Deciding later than ESA</h3><p>Orbite waited for the silent operator until its take-over deadline, 8 h before the approach. The tacit agreement now settles it at the answer deadline.</p></div>
<div class="orb-bug"><p class="by">soma check</p><h3>No State could ever be declared</h3><p>An invariant between two slots written with <code>?? 0</code> refused the very first write. Five subsystems had it; the agents fixed it from the warning.</p></div>
</div>

Building Orbite also improved Soma itself: the verifier now searches each rule over its own cone of machines, follows the success of a `try`, accepts a link key a signal makes once, and no longer mistakes two cells' handlers of the same name for a recursion. All of it shipped in [Soma 3.2.4](/changelog/3.2.4).

## Six properties, and how each is held

| # | Property | How Orbite holds it | Status |
|---|---|---|---|
| 1 | At most one standing designation per conjunction; never two avoidance manoeuvres | the lock and its four rules | **✓ proven** |
| 2 | An object without propulsion, out of service or in anomaly is never designated | the lock refuses it; any loss of capability reopens its coordinations | ◐ rules + bodies + tests |
| 3 | A crewed object manoeuvres only if it is the only one able to | checked where the lock is taken | ◐ bodies + tests |
| 4 | No manoeuvre approved without screening, on stale data, or for a suspended operator | a chain of six states, one condition from another subsystem per step | **✓ first step proven** ◐ rest open |
| 5 | Every level-3 alert answered before its deadline or escalated; silence never blocks | one clock, escalation to the States, tacit agreement, take-over | ◐ rules + replay |
| 6 | Every decision traceable to its data; data only to authorised parties | evidence frozen at the instant of the decision, controlled reads | ◐ rules + end-to-end |

The approval chain behind property 4 is the real frame of `ManPlan.approuver` (Orbite is written in French):

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

## What is not proven — yet

- **The 97 rules are proven on the skeleton**: they hold for every body that stays in its frame (`soma check` proves each one does) and takes an edge only under its condition — which `soma verify` judges on the bodies. Running it on all 1,787 bodies together does not yet finish in reasonable time; that is the next work on the verifier.
- **149 rules through a link** (`via`) stay open obligations until then: each needs the verifier to see, in the body that writes the link, that it is written once.
- **Notified is not informed**: in the replay, the silent operator receives the decision but never acknowledges it. Orbite reduces the 2019 risk — the other satellite moving too — but cannot remove it.
- **Simulated**: orbital mechanics are simplified, every actor is fictional. Orbite is a demonstration of method, not a service.

We have not built the same system in another language with the same agents to compare; that experiment is the honest next step, and we will publish its result either way.

## Check it yourself in eight minutes

Everything above can be checked with the public release. The whole program is published — 491 files, for people and for agents:

```console
$ curl -fsSL https://soma-lang.dev/install.sh | sh          # soma 3.2.4
$ curl -fsSL https://soma-lang.dev/cases/orbite/orbite.tar.gz | tar xz && cd orbite
$ soma check prod.cell                                       # 0 errors, 1,787 signals, all filled
$ soma verify app.cell                                       # about 8 minutes: VERIFY OK, 97 rules proven
$ soma test test_aeolus.cell                                 # the 2019 replay: 99 assertions
```

<div class="orb-paths">
<div class="orb-path"><h3>If you are an agent</h3><p>Fetch the files one by one, with their sha256.</p><ul><li><a href="/cases/orbite/README.md">README</a> — layout and what each command prints</li><li><a href="/cases/orbite/index.json">index.json</a> · <a href="/cases/orbite/MANIFEST.txt">MANIFEST.txt</a></li><li><a href="/llms-skeleton.txt">llms-skeleton.txt</a> — the method</li></ul></div>
<div class="orb-path"><h3>If you do formal methods</h3><p>The rules are on the real code, not on a model beside it.</p><ul><li><a href="/cases/orbite/src/sys/coordination/verrou.cell">the lock</a> and <a href="/cases/orbite/src/sys/coordination/dossier.cell">the coordination</a></li><li><a href="/cases/orbite/src/soma.toml">soma.toml</a> — the rules as tables</li><li><a href="/guide/guarantees">what <code>verify</code> proves, and what it does not</a></li></ul></div>
<div class="orb-path"><h3>If you work on space safety</h3><p>The protocol, its escalation to States, its evidence files.</p><ul><li><a href="/orbite-aeolus">Aeolus vs Starlink 44, replayed</a></li><li><a href="/cases/orbite/src/AEOLUS_TRACE.md">the replay's journal, line by line</a></li><li><a href="/cases/orbite/src/CONSIGNES.md">the six properties, as given to the agents</a></li></ul></div>
<div class="orb-path"><h3>If you want to build like this</h3><p>The same path works at any size.</p><ul><li><a href="/guide/skeleton">Large programs, skeleton first</a></li><li><a href="/learn/6-a-large-program-skeleton-first">Lesson 6</a> — step by step</li><li><a href="/agents">For AI agents</a> — the loop to run</li></ul></div>
</div>
