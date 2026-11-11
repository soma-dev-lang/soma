# Security

## Reporting

Report a vulnerability privately through
[GitHub security advisories](https://github.com/soma-dev-lang/soma/security/advisories/new).
Do not open a public issue for it.

Two kinds of report are treated as security issues:

1. **A vulnerability in the toolchain or runtime** — `soma serve` (request
   handling, routing, the bus), storage, package installation, the native
   compiler, a capability scope a model can escape.
2. **A false proof** — `soma verify` or `soma check` reporting as *proven*
   (a `✓` on an invariant, a bound, a temporal property, a refinement, a
   cost) something a program can violate. Programs are deployed on the
   strength of those lines. Include the program and the execution that
   breaks the claim; a claim reported as `runtime-checked` or with a `⚠` is
   not a false proof.

## What happens

- You get an acknowledgement, then a fix or an explanation of why the
  behaviour is intended.
- A fixed false proof is added to the [soundness log](docs/SOUNDNESS_LOG.md)
  with what was claimed, why it was false and what the prover does now, and
  gets a regression test. The log is append-only.
- A release that fixes a security issue says so in the changelog under its
  own heading.

## Deployment notes

`soma serve` binds 127.0.0.1 by default. Do not expose it on the open
internet without a reverse proxy in front of it (`soma docs operations`).
An attestation (`soma verify --attest`) records what the verifier
concluded about hashed sources with a hashed toolchain; it does not make
the verifier itself trusted.
