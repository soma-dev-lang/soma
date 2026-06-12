# Soma corpus — verified example programs

168 complete Soma programs across 10 domains, **every one passing `soma check` and `soma test`**
on the current binary. Generated as LLM training data: idiomatic, diverse,
self-verifying (each carries a `cell test` block).

| Domain | Programs | What's inside |
|--------|----------|---------------|
| algorithms | 18 | sorts, searches, BFS/DFS, DP (knapsack, LIS, edit distance), stacks/queues |
| web | 16 | request-routing CRUD, URL shortener, sessions, leaderboards, feature flags |
| agents | 15 | cell agent + bounded think() + tools + budgets + verified lifecycles |
| state_machines | 14 | order/ticket/document lifecycles, interlocks, billing — proven by verify |
| finance | 17 | ledgers with memory invariants, P&L, position limits, payoffs |
| games | 19 | tic-tac-toe, dice, RPS, Conway, card decks |
| data | 17 | filter_by/group_by/agg/pluck pipelines over record lists |
| text | 18 | tokenizers, ciphers, templating, slugify, RLE, number-to-words |
| math | 17 | vectors, matrices, statistics, root finding, primes |
| records | 17 | record literals, dot/nested mutation, entities in slots |

Verify the whole corpus:

```
for f in examples/corpus/*/*.cell; do soma check "$f" && soma test "$f"; done
```
