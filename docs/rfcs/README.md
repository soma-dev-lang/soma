# Language changes (RFCs)

Soma is learned by models from text that outlives a release, so the
language changes on a schedule the text can keep up with.

## What needs an RFC

Anything that changes what a program that passes `soma check` and
`soma test` does, or that makes `soma check` refuse a program it accepted:

- new or changed syntax, operators, precedence;
- a builtin whose result changes for existing arguments;
- a new check error (a new *warning* does not need one);
- a change to what `soma verify` reports as proven.

A fix for a false proof ([soundness log](../SOUNDNESS_LOG.md)) or a
security issue does not wait for an RFC: it ships in the next release and
is recorded there.

## Where a change may land

| Change | Lands in |
|---|---|
| Additive (new syntax that was an error, a new builtin, a new command) | any release |
| A check that only refuses programs that raise on every execution of the refused line | any release (3.0.0's expression types are the model) |
| Anything that refuses or changes a program that ran correctly | a new **edition** only |

An edition is named by its year (`edition = "2027"` in soma.toml). A
toolchain compiles every edition it ever shipped; `soma fix --edition N`
does the mechanical part of the migration and lists the rest. A program
without the key is the edition its first toolchain wrote — "2026" for
everything before 3.0.

## The process

1. Copy `0000-template.md` to `NNNN-short-name.md` and open a pull request.
2. The RFC states the diagnostics (with codes) a user will see, the
   migration, and the measured effect on the corpus: how many of the
   repository's programs change verdict, and why. "No program changes" is a
   measurement, not an expectation — run it.
3. Accepted RFCs are implemented behind the edition they name, with:
   regression tests, a `checker/codes.rs` entry per new diagnostic, the
   agent guide (`site/llms.txt` and its parts) updated in the same change.
4. The benchmark (`bench/agents`) and the backend fuzzer
   (`tools/diff_fuzz.py`) are run before a release; a release that lowers
   the benchmark's headline numbers or has a backend divergence says so in
   its notes or does not ship.

## What is versioned together

The specification (`docs/SEMANTICS.md`, `spec/grammar.md`), the
conformance material (the Rust test suite, the corpus, the fuzzer) and the
benchmark carry the toolchain's version. A second implementation is
conformant for a version when it passes that version's suite.
