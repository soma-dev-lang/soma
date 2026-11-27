# ORBITE — source, for agents and people who want to check it

ORBITE is a simulated, worldwide space traffic coordination system written in Soma,
skeleton first, by 20 coding agents. The case study: https://soma-lang.dev/orbite
States, operators and objects are fictional. The code and its comments are in French
(it was specified in French); identifiers are French too.

## Get it

    curl -fsSL https://soma-lang.dev/cases/orbite/orbite.tar.gz | tar xz     # → ./orbite/
    cd orbite

Or file by file: https://soma-lang.dev/cases/orbite/index.json lists every file with its
kind, line count, sha256 and URL; https://soma-lang.dev/cases/orbite/MANIFEST.txt is the
same as `sha256  lines  path`. Check a download with
`shasum -a 256 -c <(awk '!/^#/{print $1"  "$3}' MANIFEST.txt)` from inside `orbite/`.

## What is in it (492 files)

| kind | lines | where |
|---|---|---|
| skeleton | 21,444 | `commun/*.cell` (types, journal, clock), `sys/<subsystem>/*.cell` — memory, invariants, state machines, frames |
| bodies | 30,407 | `commun/*_chair.cell`, `sys/<subsystem>/chair/*.cell` — `flesh` blocks |
| tests | 16,981 | `test_<subsystem>.cell` (unit, with test doubles), `doublures/*.cell` (3,906), `test_bout_en_bout.cell` (end to end, no doubles) |
| entries | 582 | `app.cell` (all skeletons), `prod.cell` (skeletons + all bodies), `banc_<subsystem>.cell` (one subsystem's bodies) |
| documents | 13,397 | `CONSIGNES*.md` (the instructions each phase's agents got), `INTEGRATION*.md` (integration reports), `sys/*/NOTES.md` (each subsystem's design notes), `sys/*/facade.txt`, `sys/*/regles.toml` |

`soma.toml` holds the rules between machines written as tables; `skeleton.lock` freezes the
220 skeletons. `outils/cycles.py` checks the call graph has no cycle.

## Check it (with the current release)

    curl -fsSL https://soma-lang.dev/install.sh | sh
    soma check prod.cell                     # 0 errors (warnings are listed and expected), 1,816 signals, all filled
    soma check app.cell                      # the skeleton alone: 0 errors, every signal a hole
    for t in test_*.cell; do soma test "$t"; done
                                             # 23 suites, 6,482 assertions, 0 failures (half a minute)
    soma verify app.cell --summary           # the skeleton: a few seconds, VERIFY OK
    soma verify prod.cell --summary          # the bodies too: about twelve seconds, VERIFY OK, no ✗
    python3 outils/cycles.py | head -3       # no cycle; deepest call chain 23
    soma skeleton prod.cell CoordVerrou.poser    # the one-screen brief of any signal
    soma refs prod.cell ApiCoordination.designe  # where a signal is declared, filled, called

These figures were measured with soma 3.4.1 on the files of this archive (`MANIFEST.txt` gives
each one's sha256).

`soma verify prod.cell` reads the bodies together and proves **246 guarded entries into a state,
149 of them through a link (`via`) to another machine**, on every run of the handlers, with no
failure. What it does not prove it lists: 1,271 ⚠ lines — calls and loops whose termination it
cannot bound, size invariants and contracts on computed values, which are checked at run time
instead. `soma verify --strict` refuses the program for those.

How it got there: with soma 3.2.3 the skeleton ended in `VERIFY FAILED` — 95 rules "not explored
— more than 12 machines"; 3.2.4 searches each rule over its cone of influence first and proved
the skeleton, leaving 151 rules through a link as open obligations and the bodies out of reach
of the verifier; 3.2.10 closed both.

`test_colreg.cell` checks the rules of the road borrowed from the sea (COLREG): give-way and stand-on,
the more manoeuvrable gives way, overtaking, a franc manoeuvre, the stand-on keeps its course, relief.
`test_aeolus.cell` replays the Aeolus / Starlink 44 incident of September 2019 on the real code;
`AEOLUS_TRACE.md` is its journal, line by line. The case: https://soma-lang.dev/orbite-aeolus

## The six properties, and where to look

1. One designation per conjunction — `sys/coordination/verrou.cell` (CoordVerrou), rule
   `CoordDossier.arretee ⇐ CoordVerrou.tenu` in `sys/coordination/dossier.cell`.
2. Never designate an object unable to manoeuvre — `CoordVerrou.poser` (brief), `ApiRef.peut_manoeuvrer`.
3. A crewed object manoeuvres last — `CoordVerrou.poser`.
4. Screened, fresh, authorised before approval — `ManPlan.approuver` in `sys/manoeuvres/plans.cell`.
5. Every alert answered or escalated — `sys/escalade/`, deadlines in `sys/escalade/bareme.cell`.
6. Traceable and need-to-know — `sys/responsabilite/`, `sys/donnees/`.

`test_bout_en_bout.cell` plays them on real bodies: a launch, an avoidance, a silent operator
escalated until ORBITE arbitrates, a collision and its frozen evidence, a re-entry.
