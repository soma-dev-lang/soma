# Soma V1 example

After the V1.2 subtraction, V1 ships exactly one feature:
**deterministic record / replay**. There is one example.

| File                       | Feature                       | Try                                                          |
|----------------------------|-------------------------------|--------------------------------------------------------------|
| `02_replay_trader.cell`    | `--record` + `soma replay`    | `soma run --record 02_replay_trader.cell && soma replay 02_replay_trader.cell` |

That's it. Four other examples were deleted in V1.2 because the
features they demonstrated didn't bring value over what already
existed in the language. See `docs/V1.md` and `docs/SEMANTICS.md §4`
for the full subtraction story.

## Why replay survived

The two tests every V1 feature was held to:

  1. **Does it work end-to-end?** Replay does — it logs JSON-lines,
     re-executes, bit-compares, detects nondeterminism via
     `now()`/`random()` interception, suggests fixes.
  2. **Does it bring value over what existed?** Yes — production
     debugging via deterministic re-execution is genuinely useful and
     no other Soma primitive provides it.

The other four features (`protocol`, `prove`, `adversary`, `causal`)
failed test #2: they shipped hooks for things that weren't actually
working, or restated existing checks in fancier syntax.

## How this example works

```soma
cell trader {
    on tick(price: Int) {
        return price * 2
    }

    on run() {
        let r1 = tick(100)
        let r2 = tick(187)
        let r3 = tick(412)
        print("ticks: {r1} {r2} {r3}")
    }
}
```

Run it normally — no log written, zero overhead:

```sh
$ soma run examples/v1/02_replay_trader.cell
ticks: 200 374 824
```

Run it with `--record` — every handler invocation is logged:

```sh
$ soma run --record examples/v1/02_replay_trader.cell
[record] writing replay log → examples/v1/02_replay_trader.somalog
ticks: 200 374 824
```

Replay the log:

```sh
$ soma replay examples/v1/02_replay_trader.cell
soma replay: 4 entries from examples/v1/02_replay_trader.somalog
--------------------------------------------------------------
  #1     trader.tick(100)  ok
  #2     trader.tick(187)  ok
  #3     trader.tick(412)  ok
  #4     trader.run()  ok
--------------------------------------------------------------
replayed 4 entries: 4 ok, 0 diverged
```

Now uncomment the `now_ms()` line in the file and re-run:

```sh
$ soma run --record examples/v1/02_replay_trader.cell
$ soma replay examples/v1/02_replay_trader.cell
  divergence at entry #1: trader.tick
      args:     100
      recorded: 102
      replayed: 103
      cause:    nondeterminism in handler — calls to now_ms
      fix:      mark the handler [pure] or pass the clock as an explicit input parameter
```

That's the whole feature. Recording is opt-in at the command line
(operators decide what to record, not programmers); replay is
deterministic (any divergence is a bug in either the program or the
runtime, and we can tell you which); nondeterminism is detected at
the source (the call to `now_ms` is named in the divergence report).

## What's "in the spirit of Soma" about it

- **Recording is an operator concern, not a programmer concern.** The
  person running the program decides what to record; the person
  writing the cell doesn't have to think about it. This matches
  Soma's philosophy that operations live in CLI flags and `scale {}`
  blocks, not in handler bodies.
- **No new vocabulary.** Handler bodies are unchanged. The flag is
  the only API surface.
- **The implementation does what it claims.** No stubs, no theatre.
  111/111 existing tests still pass after V1.2 (100 use cases + 10
  CLBG + this one example), and recording overhead is zero when the
  flag is off.
