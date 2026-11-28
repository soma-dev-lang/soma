# RFC 0004 — the law says what follows, and what relates two entities

- Status: **proposed** — nothing here is implemented. Open for debate.
- Edition: any release (both forms are additions: no program that checks
  today is refused)
- Diagnostic codes added: to be numbered when accepted — one for a
  consequence no frame carries, one for a relation a condition misuses

## Problem

RFC 0003 gave a rule two formal readings: `requires Action(p): condition`
(when an action is allowed) and `always x: condition` (what holds of every
entity). A rewrite of a billing system under a law of 419 rules, 85 of them
formal (a field report, not published), met two things neither can say.

**1. A fact that entails another.** A share of that law reads "when this
happens, that follows":

- a change of subscription triggers an invoicing run;
- an invoice or a payment triggers the re-evaluation of the overdue state;
- removing the tag that suspends payments resumes the payments set aside;
- committing an invoice moves the date up to which its subscriptions are
  billed.

These stay sentences. The skeleton offers the signals (`Invoicing.trigger`,
`Overdue.trigger`), nothing obliges any signal to call them, and in that
skeleton none does: `soma check` is silent. It is the forgotten path again,
on the side of what must happen rather than what must not.

**2. A state that relates two entities.** `state name(Entity): Bool | Int`
is a fact about one entity. "The invoice of a child account whose payments
are delegated to its parent is not paid" relates an invoice to an account.
The law had to flatten it into one Bool (`invoice_delegated(Invoice)`),
whose `maps` then crosses two cells. Elsewhere the skeleton made composite
keys (`"object|tag"`, `"version|name"`) to fit two entities in one
parameter.

## Change

### 1. `entails`: a consequence, judged on the frames

```
rule "DEC-T01" "A change of subscription triggers an invoicing run"
    about   SubscriptionChange, InvoicingTrigger
    entails SubscriptionChange(a) -> InvoicingTrigger
```

Both sides are actions the law declares. What the compiler derives, before
any body exists: every signal that `realizes SubscriptionChange(…)` must
reach, through the `calls` (or an `emits` that is heard) of its frame, a
signal that `realizes InvoicingTrigger`. A check on frames, of the same
order as the closure by effects (E0138).

What it does **not** say: that the body makes the call on every path. The
frame allows; it does not oblige. So the static check establishes that the
consequence is *possible and declared*, and the obligation itself is owed
by the flesh — shown in the brief (`entails InvoicingTrigger — call
Invoicing.trigger`), and testable. Whether to go further (a monitor: after
an accepted `SubscriptionChange`, a call to the consequence was made in the
same handler) is the first open question.

### 2. A state that returns an entity

Two candidate forms:

```
state line_of(Line, Invoice): Bool            // a relation, two parameters
state account_of(Invoice): Account            // a function to an entity
```

This RFC proposes the second. A condition then reads a state of the
related entity — `not delegated(account_of(f))` — and stays without
quantifier, readable by the owner of the law. The first form invites
"there is a line such that…", which the closed language refuses on purpose.

In the skeleton, `maps account_of(f) = account.get(f) ?? ""` has the type
of the entity's key. Composition crosses cells the way a `maps` already
does: the inner state is read in the cell that maps it.

## Open questions

1. **Immediate or deferred.** "Triggers" may mean in the same handler, or
   by a job later. Does `entails` need two words, or does the deferred case
   stay a sentence?
2. **`calls` or `emits`.** An emitted signal that nobody hears satisfies
   nothing. Is an `emits` accepted only when a handler `on` it exists and
   realizes the consequence?
3. **Parameters the cause does not have.** The trigger may need what the
   cause lacks (a plugin's answer, today's date). Does `entails A(a) -> B`
   bind nothing, or `-> B(a)` bind what it can?
4. **Linking the entities.** The subscription that changed and the account
   invoiced are two entities; relating them needs form 2 above. Are the
   two changes one RFC or two?
5. **An entity-valued state that is absent.** What does
   `delegated(account_of(f))` read when `f` has no account: false, or a
   refusal?
6. **`grants`.** The manifesto's directions ask for a word for what is
   *required to be allowed* (a program that refuses everything keeps every
   `requires`). `entails` and `grants` are both obligations to act; they
   should be designed together.

## Migration

None: both forms are additions.

## Measured effect

To be measured before acceptance, on the field report's law: how many of
its 334 rules in sentences read as `entails`, and how many composite keys
and flattened states form 2 removes.

## Alternatives considered

- **Do nothing.** Consequences stay sentences served by a branch tagged
  `under`: a claim backed by its tests, with no list of the places that owe
  it. That is the state of 3.6.
- **Quantified conditions** (`exists l: line_of(l, f) and …`). Refused: the
  condition language is closed so that its owner can read it and a monitor
  can be derived from it.
