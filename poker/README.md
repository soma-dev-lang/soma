# Felt — a poker server bots play, where chips can't leak

Heads-up Texas Hold'em over HTTP. Bots POST actions; the server deals,
runs the betting, evaluates the hands, and pays the winner. What Soma
adds is the part a real cardroom cares about: **the chips are safe by
construction, not by careful code.**

```
soma verify poker/app.cell      # proves the betting machine resolves
soma test   poker/app.cell      # 8 hand-ranking + chip-safety assertions
soma serve  poker/app.cell -p 8080
python3 poker/bots.py 12        # two bots play 12 hands
```

```
start: alice=1000 bob=1000 total=2000
  hand 1: winner=bob pot=20
  ...
  hand 4: winner=alice pot=200
end:   alice=1110 bob=890 total=2000  (hands=12)
chips conserved across 12 hands: 2000 in, 2000 out ✓
```

## The three guarantees

**Chips are conserved.** Every chip movement goes through `Bank`, whose
only debit primitive (`post`) moves chips from a player into the pot —
it cannot mint or burn them. Sum of all stacks plus the pot is invariant
across every hand; the bot driver asserts it and it never moves.

**You cannot bet what you don't have.** `invariant chips >= 0` lives on
the ledger. A cheating or buggy bot that posts more than its stack has
the write *rejected* — its stack is unchanged and the bet does not
happen:

```
overbet attempt: {"ok": false, "reason": "bob cannot post 999994 on a stack of 885"}
stack intact:    alice 1100 bob 885 total 2000
```

This isn't validation code that might have a hole — the negative balance
is unrepresentable, so there is no hole.

**The hand always resolves.** The betting streets (`preflop → flop →
turn → river → showdown → complete`, plus a fold edge from every street)
are a state machine `soma verify` proves reaches the terminal `complete`
from every state — no hand can hang half-dealt.

## The poker is real

A genuine 7-card evaluator scores all eight hand classes with kickers
and the wheel (A-2-3-4-5) straight, unit-tested against the canonical
ordering (royal flush > quads > full house > flush > straight > trips >
two pair > pair > high card). Blinds, position, the big-blind option,
and showdown all work — the bot transcript above is real play.

## The HTTP API

| Endpoint | Effect |
|----------|--------|
| `GET /join?name=X` | take a seat (1000 chips) |
| `GET /deal` | start a hand |
| `GET /state?seat=N` | your hole cards, the board, pot, whose turn, the bet |
| `GET /act?seat=N&move=fold\|check\|call\|raise&amount=A` | act |
| `GET /table` | stacks, pot, total chips in play |
| `GET /` | a lobby page |

Bots are any process that can speak HTTP; `bots.py` is ~90 lines.

## Note

Heads-up (two seats), no side pots — the betting loop is exact for two
players. Multiway with side pots is the natural next step and doesn't
change the safety story: the same `Bank` invariant guards any number of
stacks.
