# Lift — a smart elevator with a proven interlock

An elevator is two problems in one cabin: **scheduling** (which floor
next?) and **safety** (never move with the doors open). `lift` does the
first with the classic SCAN algorithm and lets the language *prove* the
second.

```
soma verify elevator/app.cell    # proves the door/motion interlock
soma test   elevator/app.cell    # pins scheduling + the refused unsafe moves
soma run    elevator/app.cell run_sim 8
```

## Scheduling — the SCAN "elevator algorithm"

Keep moving in one direction, serving every call on the way, until
nothing waits ahead; then reverse. It's the algorithm disk I/O
schedulers are named after, and it's why a real elevator doesn't
ping-pong. The signature shows in the metrics: **6 hall calls served in
~6 floors of travel** — one sweep of the shaft, not the sum of pairwise
trips a naive "nearest call" scheduler would pay.

```
LIFT simulation
  hall calls:   6
  served:       6
  travel:       6 floors      <- one sweep, not 15+
  avg wait:     5.2 ticks
```

## Safety — the interlock is a theorem, not a wire

The motion state machine has **no state that is both moving and
doors-open**, and **no transition from a moving state into an open-door
state**. The cabin must reach `idle` (a full stop) before any door
moves, and must close its doors back to `idle` before it can move again:

```
idle -> moving_up      moving_up -> idle      (a moving car can only stop)
idle -> doors_opening  doors_open -> doors_closing  (an open door can only close)
```

So "move with the doors open" is **unreachable** — `soma verify`
confirms it over all 6 reachable states with no deadlock. Every move and
every door cycle in the simulation runs *through* this machine, so the
interlock is exercised on every tick, and the tests pin the refusals:

```soma
assert_fails transition("c1", "moving_up")    // from doors_open — raises
assert_fails transition("c2", "doors_open")   // from moving_up  — raises
```

A V1.8 invariant keeps the cabin in the shaft regardless of any
scheduling bug:

```soma
invariant floor >= 1 && floor <= 10   // a computed floor of 11 is rejected at the write
```

## Why this is the showcase

The scheduler can be arbitrarily clever — SCAN today, a learned policy
tomorrow — and the safety property does not depend on it being correct.
In a PLC the interlock is wiring you hope matches the logic; here the
unsafe state is absent from the model, so no scheduler, however buggy,
can reach it, and `verify` is the proof. Cleverness and safety live in
separate layers, and only one of them has to be trusted.
