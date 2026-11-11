# RFC NNNN — title

- Edition: (any release | 20NN)
- Diagnostic codes added: (none | E…)

## Problem

What goes wrong today, with a program that shows it and the output it gets.

## Change

The rule, stated as a user would read it in the guide. Then the exact
diagnostics: text, code, the `fix` clause.

## Migration

What `soma fix --edition` rewrites mechanically, and what it leaves to the
author with which diagnostic.

## Measured effect

- Corpus and repository programs that change verdict under the change: N of M, listed.
- Agent benchmark (`bench/agents`), before / after, if the change is meant to help convergence.
- Backend fuzzer: comparisons run, divergences.

## Alternatives considered

Including "do nothing", and why not.
