# orbite-core — the core of the ORBITE protocol, proven whole

When two objects in orbit are about to collide, exactly one of them must move, and everyone must
know which one. ORBITE is a (simulated) protocol that decides it, with rules of the road borrowed
from the sea (COLREG 1972). The full version (https://soma-lang.dev/cases/orbite/) is 69,000 lines written by 20
agents; nobody can review it and `soma verify` does not finish on its 1,787 bodies together.

**orbite-core** is its core: one Soma program of about 1,300 lines (plus 600 lines of tests) that a
person can read in an evening, in which **every rule between machines is proven by `soma verify`
on the complete program, bodies included** — not only on the skeleton.

All names in the code are French, like the full version. Everything is fictional except the 2019
Aeolus/Starlink 44 timeline used by the replay.

## What it contains

One entry file, `app.cell`: the skeleton first (memory, invariants, state machines, frames with
the rules on their edges), then the bodies (`flesh`).

| Cell | Machine | What it does |
|---|---|---|
| `Journal` | — | append-only log (`[immutable]` list): every decision is traceable |
| `Roles` | — | who acts: `systeme`, `analyste:<name>`, `superviseur:<name>`, `operateur:<org>`; four eyes |
| `Operateurs` | `inscrit → actif ⇄ suspendu → radie` | operators |
| `Catalogue` | `inscrit → manoeuvrant → inerte → hors_service` | objects: owner, propulsion (none / low-thrust / impulsive), crewed, launch date, mean altitude |
| `Priorites` | — | the rules of the road, a pure function `qui_manoeuvre` (who gives way) |
| `Rapprochements` | `detecte → alerte → en_coordination → resolu \| passe` | conjunctions from the screening engine: TCA, distance, collision probability (billionths), relative speed, levels 1–3 |
| `Verrou` | `free → held → lifted → held … → closed` | the lock: one designated object and one stand-on object per conjunction |
| `Coordination` | `ouverte → proposee → accord \| arbitrage → arretee → liberee → rouverte → … → close` | proposal, answers, **tacit agreement**, four-eyes arbitration, reopening, **relief (rule 17b)** |
| `Plans` | `declare → crible → (habilite → capable) → approuve → execute → verifie \| annule` | avoidance manoeuvre plans |
| `Escalade` | — | a silent operator past the deadline: breach, then levels 1 → 3 |
| `Horloge` | — | the only entry of time: `balayer(h)` moves every deadline |
| `Orbite` | — | the three gestures that cross cells (signal, verify a burn, reopen) |

The rules of the road (`Priorites.qui_manoeuvre`): an object without propulsion never moves; a
crewed object moves only if it is the only one able to; otherwise the more manoeuvrable gives way
(no propulsion › crewed › low-thrust › impulsive, rule 18); between equals, when one overtakes the
other (relative speed < 100 m/s) the lower one gives way (rule 13), else the last launched (rule
15); and an object is able only if its notice (6 h impulsive, 72 h low-thrust) still holds before
the TCA. A plan is approved only if the burn is decisive (residual probability < 1 in 1,000,000,
rule 8), and never for the stand-on object (rule 17a).

## The rules between machines (13, all proven on the full program)

Written on the edges of the frames (`moves a -> b when Other.state [via link]`) or in `soma.toml`:

| # | Rule | Property |
|---|---|---|
| 1 | `Verrou: free → held` only while `Coordination` is `accord` or `arbitrage` | 1 |
| 2 | `Verrou: lifted → held` only while `Coordination` is `accord` or `arbitrage` | 1 |
| 3 | `Verrou: held → lifted` only while `Coordination` is `liberee` | 1 |
| 4 | `Coordination: accord → arretee` only while `Verrou` is `held` | 1 |
| 5 | `Coordination: arbitrage → arretee` only while `Verrou` is `held` | 1 |
| 6 | `Coordination: liberee → rouverte` only while `Verrou` is `lifted` | 1 |
| 7 | `Coordination: ouverte → proposee` only while `Rapprochements` is `en_coordination` | 5 |
| 8 | `Coordination: ouverte → arbitrage` only while `Rapprochements` is `en_coordination` | 5 |
| 9 | `Plans: crible → habilite` only while the plan's operator is `actif` (via `operateur_de`) | 4 |
| 10 | `Plans: habilite → capable` only while the plan's object is `manoeuvrant` (via `objet_de`) | 2 |
| 11 | `Plans: capable → approuve` only while the lock of the plan's conjunction is `held` (via `rapprochement_de`) | 1 |
| 12 | `Plans.approuve` never before `Plans.crible` (soma.toml) | 4 |
| 13 | `Coordination.arretee` never before `Rapprochements.en_coordination` (soma.toml) | 5 |

Rules 1–6 are the four rules of the Lock/Coordination miniature on the Soma site, for the real
bodies. The three `via` links are written once, when a plan is declared
(`require objet_de.get(plan) == () else Existe`), and never changed.

## How to check it

```console
$ curl -fsSL https://soma-lang.dev/install.sh | sh     # soma 3.2.4
$ curl -fsSL https://soma-lang.dev/cases/orbite-core/orbite-core.tar.gz | tar xz && cd orbite-core
$ S=soma
$ $S check .
check: 3 file(s), 0 failed
$ $S verify app.cell                          # about 2 seconds
  ...
  ✓ entering Verrou.held from free while Coordination.accord | Coordination.arbitrage — every run of the handlers, 12 product state(s) explored over Verrou.verrou (4) × Coordination.coordination (8)
  ...  (13 lines "✓ entering …" / "✓ before …" under `product:`)
108 passed, 0 warnings, 0 failures
  ...
VERIFY OK
$ $S verify --strict --summary app.cell
114 proven (✓), 1 note (·) — `soma verify` without --summary prints them
VERIFY OK
$ $S test test_colreg.cell                    # each rule of the road, applied and not applied
31 tests: 31 passed, 0 failed
$ $S test test_core.cell                      # properties 1–4 as explicit refusals, tacit agreement, relief
165 tests: 165 passed, 0 failed
$ $S test test_aeolus.cell                    # the 2019 replay
48 tests: 48 passed, 0 failed
```

`verify` also proves, per machine, reachability, absence of deadlock, liveness, refinement and
termination of every handler, every memory invariant (23 writer proofs, none left to run time) and
the two `ensures` of `Rapprochements.niveau_pour`. The one `·` note says that
`Coordination.clore` takes "any edge into `close`" (`transition(r, "close")`, all seven edges
listed in its frame) instead of naming the source; it is not a warning and `--strict` passes.

A rule that is false is found: remove the `require Verrou.tenu(r)` from `Plans.approuver` (or any
of the eight guards the rules rely on) and `verify` names the run that breaks it — after 3 to 14
minutes, because a failing rule is searched again over the product of all six machines.

## The 2019 replay (`test_aeolus.cell`)

Same historical facts and simulated parameters as `https://soma-lang.dev/cases/orbite/src/test_aeolus.cell`: TCA 2 Sept 2019
12:00 UTC; alert at 1 in 50,000 on 28 Aug (level 2, no coordination yet); up to 1 in 1,000 on 30
Aug at 14:02 (level 3: coordination opened, Aeolus proposed — Starlink 44's low-thrust drive can no
longer give 72 h notice); ESA accepts at 14:10; SpaceX stays silent; at 15:00 the deadline passes
and the **tacit agreement** settles it (the operator that must move said yes, the other said
nothing), SpaceX gets a breach and climbs escalation levels 1 → 3; at 15:20 a 0.5 m/s plan is
refused as too timid (residual 1 in 730,000), a 0.7 m/s plan is approved; any Starlink 44 plan is
refused; burn on 2 Sept at 11:15:10; orbit verified at 11:50, conjunction resolved, coordination
and lock closed. The test ends with the conjunction's chronology read back from the journal.

## Relation to the full version

orbite-core keeps the decision chain of `https://soma-lang.dev/cases/orbite/` — catalogue, conjunctions, rules of the
road, coordination, lock, plans, minimal escalation, one clock — and drops everything around it:
the HTTP gateway, detailed access rights, ephemerides, observation, space weather, launches,
re-entries, end of life, liability, data sharing, compliance ratings, operations. Where the full
version proves its 97 rules on the skeleton only (bodies read as their frames) and leaves 151
`via` rules as open obligations, the core's 13 rules are proven with every body in place.

What the core does not cover, compared with the full version:
- no ephemerides, so property 4's "not on stale data" is out of scope (screening and operator status are in);
- no observation: a burn is "verified" by an orbit offset given by the caller;
- no counter-proposal round: a refusal goes straight to arbitration;
- no multi-object clusters (one object in several simultaneous conjunctions is handled only by rule 17a's counter);
- escalation stops at level 3 (no States, no diplomatic channel), and the operator is notified only through the journal;
- data diffusion and evidence files (property 6) reduce to the append-only journal.

See `NOTES.md` for the design decisions and what is proven versus tested.
