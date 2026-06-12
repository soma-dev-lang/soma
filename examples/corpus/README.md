# Soma corpus — verified example programs

**316 complete Soma programs across 20 domains — every one passing
`soma check` and `soma test`** (and `soma verify` where a state machine
exists) on the current binary, independently re-verified before commit.
Generated as LLM training data: idiomatic, diverse, self-verifying, and
(wave 2) story-driven — each safety program names the real-world failure
it makes unrepresentable (Therac-25, Lauda Air 004, Helios 522,
Überlingen, Apollo 13, Ladbroke Grove, ...).

| Domain | Programs |
|--------|----------|
| aerospace | 15 |
| agents | 15 |
| algorithms | 18 |
| civic | 14 |
| data | 17 |
| devops | 14 |
| energy | 15 |
| escrow_finance | 15 |
| finance | 17 |
| games | 19 |
| games_economy | 16 |
| governance | 16 |
| logistics | 15 |
| math | 17 |
| medical | 14 |
| records | 17 |
| safety_interlocks | 14 |
| state_machines | 14 |
| text | 18 |
| web | 16 |

Verify the whole corpus:

```
for f in examples/corpus/*/*.cell; do soma check "$f" && soma test "$f"; done
```
