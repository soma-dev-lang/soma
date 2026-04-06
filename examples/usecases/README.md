# Soma Use Cases — 100 Verified Examples

Every example in this directory:
- Passes `soma check` (contracts verified)
- Runs with `soma run` (produces correct output)
- Demonstrates a real Soma feature

## Categories

| # | Category | Examples |
|---|----------|---------|
| 1-10 | Basics | Hello world, variables, control flow, types |
| 11-20 | Web & API | REST APIs, routing, HTMX, SSE |
| 21-30 | Data Pipelines | Pipes, filter, map, reduce, analytics |
| 31-40 | State Machines | Workflows, lifecycle, verification |
| 41-50 | AI Agents | think(), tools, multi-agent, delegation |
| 51-60 | Math & Algorithms | BigInt, recursion, sorting, search |
| 61-70 | Real-World Apps | Todo, chat, queue, cache, config |
| 71-80 | Finance | Pricing, portfolio, risk, exchange |
| 81-90 | Pattern Matching | Destructuring, guards, ranges, routing |
| 91-100 | Advanced | Native compilation, multi-file, testing |

## Run all examples

```bash
for f in *.cell; do echo "=== $f ===" && soma check "$f" && soma run "$f"; done
```
