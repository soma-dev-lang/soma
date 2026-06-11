# Airlock — safety by construction

A two-door airlock controller (spacecraft / BSL-4) where the one fatal
mistake — **both doors open at once** — is not guarded against but made
*unrepresentable*, and the verifier proves it.

```
soma verify airlock/app.cell    # proves the reachable state set is the 6 safe states
soma test   airlock/app.cell    # pins every refused unsafe command
soma run    airlock/app.cell demo
```

## The safety argument

This is how safety-critical state machines are *supposed* to be built —
"correct by construction" — and Soma lets you actually prove it:

1. **The breach state does not exist.** `inner_open` has the outer door
   sealed; `outer_open` has the inner door sealed. There is no state in
   which both doors are open, so no command sequence can produce one.
2. **`soma verify` confirms the reachable set.** It proves all 6 states
   reachable from `sealed`, proves no deadlock (you can never get stuck
   in a depressurized lock), and recognizes the lock as a reactive
   system that can always cycle back to safety.
3. **The tests pin the refusals.** `assert_fails transition("t1",
   "inner_open")` from `outer_open` is the mutual-exclusion property in
   executable form — the controller raises rather than breach.
4. **Continuous safety via V1.8 invariants.** `invariant pressure >= 0
   && pressure <= 101` means the seals never see a negative or burst
   pressure, regardless of the command stream — a breach write is
   rejected at the memory and the slot keeps its last safe value.

## Why this is hard anywhere else

A PLC or C interlock encodes "both doors never open" as wiring you hope
is right, validated by testing the sequences you thought of. The bugs
live in the sequences you *didn't* think of. Here the unsafe state is
absent from the model, so there is no sequence — thought of or not —
that reaches it, and `verify` is the proof object you hand the safety
auditor. Every refused command quotes the machine's valid transitions,
so the controller is self-documenting under attack:

```
REFUSED: invalid transition: outer_open → inner_open.
         Current state: 'outer_open'. Valid targets: [vacuum]
```
