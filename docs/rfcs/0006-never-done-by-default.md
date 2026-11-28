# RFC 0006 — Soma never lets an agent believe it is done (4.0.0)

- Status: **implemented on the working branch, not released** — parts A, B and C. Awaiting the owner's decisions below and the release of 4.0.0.
- Edition: any release for what is added (`entry`, `internal`, `held`,
  `--law`). What changes for every caller — the last line of `check` and
  `verify`, the exit statuses of `soma next` — is why this is a major
  version.
- Diagnostic codes added: E0142 to E0145.
- Source: a request written from a field project (a billing system
  rewritten under a law of 419 rules; 249 signals, no body yet), completed
  by a second agent who measured each claim. This RFC takes that request
  whole: one sentence, three parts. It does not split it into features.

## The sentence

**Soma never lets an agent believe it is done, and gives it one thing to do
in a perimeter it can read whole.**

## Why

On the field project `soma check` said "no errors" and `soma verify` said
`VERIFY OK`. A rule-by-rule rereading by agents who had not written the
skeleton then found 116 rules of 419 misrepresented; after correction, a
second rereading found 109 (39 of substance, 70 owed without object). The
agent announced the skeleton "finished" twice, on a green check.

What the agent says it does when it has more than it can read:

1. it delegates, and the surplus comes back as reports;
2. it counts, and a hundred findings become three numbers;
3. it takes the next step of the loop, which is always possible;
4. it holds on to a green check.

None of these needs understanding. Each time the work was stepped back
from, the owner of the project asked for it; the tool never did. The three
parts answer the reflexes a tool can answer: the verdict (4), `soma next`
(3), the perimeter (1 and 2).

## Part A — the verdict says what was not judged

### The last line

`soma check` and `soma verify` end, for a program under a law or with a
skeleton, on a line that says what is established **and what is not**:

```
conforms — fidelity not judged: 334 rules in sentences (no tool reads them),
26 silent signals, 249 signals neither entry nor internal, 84 signals nobody
calls, 0 events nobody hears, 2 slots read only by their writers
```

Two kinds of numbers, kept apart in the line:

- **what the tool cannot judge**, said as such: rules in sentences;
  `silent` signals (declared, reviewed, not proven); signals that say
  neither `entry` nor `internal`;
- **what it computes on the frames, with no body**, and that makes
  fidelity impossible: a signal nobody calls, an event nobody hears, a slot
  read only by its writers, a second path, a held state one of its sources
  can make stale.

"All checks passed" and a bare `VERIFY OK` are no longer the last line of
such a program. A program with no law and no skeleton keeps today's lines.
`--json` carries the same numbers under `not_judged`.

### Three clauses

To compute the last two, a skeleton says what it says today only in
comments.

In a frame, on its own line like `silent "why"`:

```soma
// Finds an account by its id or its external key.
signal find(account: Account) -> AccountCard
    entry
    reads known, cards
    realizes AccountLookup(account)

// The card of an account; the empty card for an unknown one.
signal card(account: Account) -> AccountCard
    internal "the caller already holds the account"
    reads known, cards
```

- `entry` — called from outside the program.
- `internal "why"` — called only by signals of the program, on an entity
  the caller already holds. The reason is required, as on `silent` and
  `exempt`: it is what a reviewer judges.

In `memory`, beside the invariants:

```soma
held remaining from lines, linked_to by recompute
```

- `held <slot> from <sources> by <signal>` — the slot is derived: `signal`
  rebuilds it from the sources. A source may be in another cell
  (`from Invoices.cards`).

All three are contextual words: no program that parses today stops
parsing. All three enter `skeleton.lock`.

### What is computed from them

| Check | On | Said as |
|---|---|---|
| a signal nobody calls | an `internal` signal in no `calls`, heard by no `emits` | a warning, and a number of the line |
| an event nobody hears | an `emits` with no signal of that name | a warning (it exists today) and a number |
| a slot read only by its writers | a slot in `writes` and in no other signal's `reads`, no `maps`, no invariant | a number, never a fault: `writes` allows reading |
| a second path | an `entry` that takes an entity the law guards, and neither `realizes` an action on that parameter nor has a signal that does in its `calls` | a warning in this edition |
| a held state that can go stale | a signal that `writes` a source of a `held` slot and is neither its holder nor has the holder in its `calls`; a signal other than the holder that writes the held slot | a warning in this edition |

"An entity the law guards": an entity that some action with a formal rule
takes as a parameter. An `internal` signal owes nothing; each of its
callers inherits what it would have owed.

### What this does not see, measured

On the field project the five checks would have reported **16 of 76**
substantive gaps of the first rereading and **10 of 39** of the second: a
fifth to a quarter. The rest — a frame too short for what the comment
says, a comment that contradicts the rule, a rule owed by one signal while
another does the thing, a type missing a field — needs a reader. The line
says "fidelity not judged" for that reason, whatever the five numbers are.

The frame allows and does not oblige (RFC 0004 says the same of
`entails`): a `calls` to the holder establishes that the refresh is
declared and possible, not that every path makes it.

## Part B — `soma next` gives one thing, and can say "stop"

### The queue

| Rank | What `next` gives | For | Exit |
|---|---|---|---|
| 1 | a gap in the agent's setup | the agent | 1 |
| 2 | an error of `check` | the agent | 1 |
| 3 | a failure of `verify` | the agent | 1 |
| **4** | **before the freeze: what the skeleton does not state, one at a time — the mirror's, then Part A's** | the agent | 1 |
| **5** | **a question for the owner of the law** | the owner | **3** |
| 6 | what the law asks: a rule not served, a formal rule with no test | the agent | 1 |
| 7 | the next hole, among those that owe no rule under a question | the agent | 1 |
| 8 | a failing test | the agent | 1 |
| 9 | `complete` | — | 0 |

- **Exit 3 means "this is not yours".** Exit 2: the program does not load
  (today mixed with "something to do"). `--json` has a `kind` of its own
  for a question.
- A question blocks only what depends on it: while ranks 6 to 8 have work
  that touches no rule under a question, `next` gives it and says in one
  line how many questions are open. Exit 3 only when all that is left
  depends on one.
- `complete` is never answered while a question is open.
- Rank 4, one at a time: today's single line ("N things not stated") was
  the counting reflex again.

### The questions

Decidable on the law and the frames, and not tasks for the agent:

- a state of the law that only `silent` signals change;
- a term realized by signals that each serve only part of its rules (the
  cause of debts without object).

The request looked for a third family in 38 verdicts of the rereadings and
found none: what is left comes from meaning — an ambiguous sentence, two
sentences that collide — and no program asks it.

### Where the owner answers

- An answer that changes the law goes in the `.law`: the question closes by
  itself at the next lock, because the fact that raised it is gone.
- "It is meant" goes in a file of decisions beside the laws
  (`lois/decisions.toml`): the question's id, the answer in one sentence,
  who answers, the date. The file enters `law.lock` like a law. A question
  closes when a decision carries its id, and reopens if the fact changes.

**A limit, left where the request left it.** Today `soma law --lock` runs
for an agent without resistance: nothing prevents it from writing "it is
meant" in the owner's name and locking. What makes that visible is that
the decisions file is short, dated, and changes the fingerprint of the
law. Whom a lock belongs to — roles — is RFC 0005 (a proposal, not
published yet), and is not part of 4.0.0: the request put it outside, and
it is decided there, whole.

## Part C — a perimeter: one law at a time

One option, common to `check`, `verify`, `law`, `next`, `skeleton --index`
and `doctor --program`:

```
soma next app.cell --law lois/invoice_generation.law
```

It restricts everything to what a law touches:

- its rules;
- the signals that serve, owe or are exempt from one of them, and their
  cells;
- the types those signals and the slots they touch name;
- **the frontier, both ways**: what those signals call outside the
  perimeter (signature, and hole or body), and what the outside must call
  inside for a held state to stay right — the second is known only where a
  slot is declared `held`;
- the verdict of Part A, for the perimeter alone. A formal rule whose
  state is held from outside says so: "state held from outside the
  perimeter by 4 signals".

Measured on the field project: thirteen laws of fourteen fit under 1,000
lines with their types (the whole file is 5,937); the one that does not
sews the others together and comes last. Types weigh as much as signals: a
perimeter that does not restrict them does not fit a screen.

### A freeze by perimeter

For: a vertical slice writes bodies in one domain while the others still
move; one late domain no longer blocks the freeze of all.
Against: perimeters share their frontier; `[verify]` properties are
global; one fingerprint is what an attestation cites.

The request left this undecided, and so does 4.0.0: **the freeze stays
global.** A middle way exists for a later version — one `skeleton.lock`,
which already has an entry per skeleton, and `soma skeleton --freeze --law
<file>` that freezes the cells of the perimeter and the signatures of its
frontier.

## What changes for whoever calls Soma

| Change | Who is touched |
|---|---|
| the last line of `check` and `verify`, for a program under a law or with a skeleton | anything that matches `All checks passed` or the exact `VERIFY OK` line; `--json` keeps `passed` |
| `soma next`: exit 2 (does not load) and 3 (for the owner) | a harness that reads "non-zero = something to do" |
| new warnings: second path, stale held state, an `internal` nobody calls | none is an error in this edition |

No program that checks today is refused.

## Order of work

Each step lands with its tests, red first, and with the field project's
numbers as acceptance.

| Step | Content |
|---|---|
| 1 | Part A without declarations: the counts on the frames, the last line of `check` and `verify`, `not_judged` in `--json` |
| 2 | The three clauses: parser, frames, lock, brief, mirror; the second path and the held state |
| 3 | Part B: the queue, the questions, exits 2 and 3, the decisions file in `law.lock` |
| 4 | Part C: `--law` on the six commands, the frontier both ways |
| 5 | The starters (`soma init --law` declares `entry`, `internal`, `held`), the guides, the one-screen pages, the site; release 4.0.0 |

## Held when

On the field project, without changing a file:

1. The last line of `soma check` contains 334, 26 and 249, and is not
   "no errors" alone.
2. `soma law --questions` lists the questions for the owner and exits 3;
   the first exists today: a state only `silent` signals change. `soma
   next` hands out what waits for no answer, and each of its answers says
   how many questions are open and the command that lists them; it answers
   `question`, exit 3, only when all that is left waits for one. (The
   request's criterion read "next gives a question and exits 3"; its author
   corrected it to this, which is what its part B says.)
3. `soma next --law lois/facture_generation.law --json` gives a target
   that is one of that law's 49 rules or 13 signals; `soma skeleton --index
   --law` of the same lists 13 signals in 5 cells and an empty frontier;
   for `lois/retard.law`, 9 signals and a frontier of 13 signals in 7 other
   cells, each with its signature and whether it has a body.

And with the declarations written: signals nobody calls go from 84 to 8;
second paths to 0 once the 11 signals of the measure are declared.

## Left out, on purpose

- The difference between two skeletons: it serves merging the work of
  several agents, which Part C aims to make unnecessary.
- "When this happens, that follows" in the law: RFC 0004. Part A covers
  its symptom (a trigger nobody calls) without waiting for the language.
- A lexical check of the sentences (a word a rule uses and no `term`
  defines).

## Outside the request, seen on the way

One line each; none is in 4.0.0.

- A freeze by perimeter (`soma skeleton --freeze --law …`): the freeze stays global.
- Roles — whom a lock belongs to: RFC 0005 (not published yet). Their code is kept on a branch.
- The harness of the replication tests reserves its ports by opening and releasing them: one run in thirty fails on "address already in use", in 3.6.3 as in 4.0.0.
- The thresholds of the question `broad-term` (three signals, three rules in sentences) are a choice without a measure.
- The second path, applied to every entity parameter of an `entry` whose type is shared by an id and an external key: asked for 4.1 by the field project.

## For the owner of the language to decide

1. **The words** `entry`, `internal`, `held … from … by`.
2. **The last line**: its wording, and whether it replaces `VERIFY OK` or
   follows it.
3. **The decisions file**: its name and its place (`decisions.toml` beside
   the first law file).
4. **The freeze by perimeter**: not in 4.0.0; the middle way, later, or never.
5. **Whether a signal that says neither `entry` nor `internal` becomes an
   error in the next edition.** This RFC proposes: counted, never refused.
