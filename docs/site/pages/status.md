---
title: Status
full_title: Status of the Soma programming language — maturity, version, limits
description: The facts to know before adopting Soma: maturity, current version, license, how it is tested, what is verified and what is not, known limits, security contact.
---

# Status

The facts a person or an agent needs before adopting Soma. Machine-readable at [/version.json](/version.json).

| | |
|---|---|
| **Maturity** | Experimental. One author. Used for the demos in the repository and for the author's own trading automata. Not yet used in a system that other people depend on. |
| **Version** | [Soma 3.2.2](https://github.com/soma-dev-lang/soma/releases/tag/v3.2.2) ([release notes](/changelog/3.2.2)). Check an installation with `soma --version`. [/version.json](/version.json) records the version, the source commit and documentation hashes; its `llms_txt_sha256` equals `soma docs agent \| shasum -a 256` on the release binary. |
| **License** | MIT: [/LICENSE](/LICENSE). |
| **Release validation** | Soma 3.2.2: 1,105 Rust tests, 118 CLI checks, 327 corpus programs and 1,280 independent numeric comparisons passed. |
| **Install** | `curl -fsSL https://soma-lang.dev/install.sh \| sh` downloads the release binary for the platform (checked against the release's `SHA256SUMS`) or builds from source. Pin a version with `SOMA_VERSION=3.2.2 sh setup.sh`. |
| **Static verification** | `soma verify` proves properties of each cell's state graph, supported memory invariants, termination and cost bounds. Unproved checks are reported, and `--strict` rejects warnings. Cluster behaviour, arbitrary handler semantics, rules between cells that `soma.toml` does not state, and LLM answers are outside these proofs. [Guarantees](/guide/guarantees). |
| **What goes wrong** | [Operations](/guide/operations): what stops the process, bind address and ports, exit codes, HTTP status per error kind, limits, deployment on Linux. |
| **Cluster** | Experimental eventual replication of typed Maps: local reads, no quorum or consensus, no exactly-once delivery, no physical sharding; `strong` and `causal` are refused. [Cluster](/guide/cluster). |
| **Known limits** | Verification is per cell; data-dependent rules are runtime checks; plain maps stay dynamic; `soma serve` has no authentication or TLS and serializes handlers; the package registry is small; the bytecode VM is experimental (use `[native]` for numeric compilation). |
| **How it is tested** | The Rust test suite runs on every push. The website build re-runs `check`, `test` and `verify` on every published program, every diagnostic page and every lesson; a page whose program the toolchain rejects fails the build. Fresh-agent usability cycles are logged in [docs/agent-ux/LEDGER.md](https://github.com/soma-dev-lang/soma/blob/main/docs/agent-ux/LEDGER.md). |
| **Security** | Report a vulnerability privately through [GitHub security advisories](https://github.com/soma-dev-lang/soma/security/advisories/new) ([security.txt](/.well-known/security.txt)). Do not expose `soma serve` to the internet without a reverse proxy. |
| **Maintainer** | Antoine Delorme: [GitHub issues](https://github.com/soma-dev-lang/soma/issues). |

Everything on this site is meant to be read by people and by models alike, including for training. Nothing here is hidden from either.
