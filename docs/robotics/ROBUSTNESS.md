# Robotics robustness audit — 100 scenarios

These scenarios exercise Soma behavior relevant to robot controllers. They do not
qualify hardware, real-time performance or a lunar system. Each scenario is a
separate Rust integration test in `compiler/tests/robotics_robustness.rs`.

## Results — Soma 2.8.3, 2026-09-20

- Robotics campaign: **100 passed, 0 failed, 0 ignored**.
- Complete Rust suite: **523 passed, 0 failed, 0 ignored**.
- Core CLI fixtures: **118 passed, 0 failed**.
- Published corpus: **321 programs passed the website build checks**.

Measured locally on macOS ARM64. Linux CI reruns the full suite on every push
and pull request. This campaign confirmed the tested behavior without finding
an additional runtime defect. The source of every scenario is in
[robotics_robustness.rs](https://github.com/soma-dev-lang/soma/blob/v2.8.3/compiler/tests/robotics_robustness.rs).

## Scope and reproduction

Added in Soma 2.8.3 on 2026-09-20. This is a regression campaign for the
language, with inputs chosen for robot-controller use. The motor and sensor
values are simulated data; no physical hardware is driven.

Run all 100 independently named scenarios from the repository root:

```sh
cargo test --release --locked --manifest-path compiler/Cargo.toml --test robotics_robustness
```

Run them with every existing Rust test:

```sh
cargo test --release --locked --manifest-path compiler/Cargo.toml --no-fail-fast
```

IDs 001–080 cover numeric boundaries, sensor validation, mission transitions,
storage invariants, rollback and collection/JSON boundaries. IDs 081–090 test
the distinction between static rejection, proof, runtime checks and unproved
distribution. IDs 091–095 run native handlers, including comparison with the
interpreter. IDs 096–099 use fresh processes to check committed data and
rollback persistence; 100 distinguishes recorded errors from returned data.
`verify_fail` and `check_fail` mean the expected rejection is asserted, including
its diagnostic. A refusal is a passing test, not a waived failure.

The separate existing `compiler/tests/cluster.rs` suite covers actual network
replicas, seed failure, reconnect, stale updates, restart, and partition recovery.
The 100 scenarios below do not add consensus or distributed safety proofs.

## Limits

These are finite examples, not a proof for every input. They do not measure
worst-case execution time, memory exhaustion, radiation tolerance, sensor or
actuator faults, abrupt power loss, or hardware fail-safe behavior. Restart tests
exit processes normally after a commit or a rejected handler; they do not inject
power loss during a disk write. Local transaction rollback cannot undo a command
already sent to a physical actuator. The cluster remains experimental and
eventually consistent, with local reads and no fenced scheduler. Use the
[guarantees](https://soma-lang.dev/docs/guarantees.md) and
[cluster limits](https://soma-lang.dev/docs/cluster.md) when assessing a design.

| ID | Scenario | Check |
|---|---|---|
| 001 | encoder counts above float precision | runtime |
| 002 | odometer integer promotion | runtime |
| 003 | negative odometer promotion | runtime |
| 004 | wide distance products | runtime |
| 005 | large finite calibration ratio | runtime |
| 006 | negative fixed point division | runtime |
| 007 | subnormal measurement ratio | runtime |
| 008 | nearby large sensor variance | runtime |
| 009 | high dynamic range median | runtime |
| 010 | exact message sequence sorting | runtime |
| 011 | divide zero is catchable | runtime |
| 012 | integer divide zero is catchable | runtime |
| 013 | remainder zero is catchable | runtime |
| 014 | nan rejected by sensor guard | runtime |
| 015 | positive infinity rejected by sensor guard | runtime |
| 016 | negative infinity rejected by sensor guard | runtime |
| 017 | nan cannot become integer command | runtime |
| 018 | huge integer cannot silently become infinity | runtime |
| 019 | non numeric actuator math is rejected | runtime |
| 020 | failed math does not poison later commands | runtime |
| 021 | sensor guard accepts exact endpoints | runtime |
| 022 | sensor guard rejects out of range | runtime |
| 023 | missing telemetry is not zero | runtime |
| 024 | false telemetry is not missing | runtime |
| 025 | zero measurement survives coalescing | runtime |
| 026 | control vector arithmetic | runtime |
| 027 | vector shape mismatch is rejected | runtime |
| 028 | large sequence dedup is exact | runtime |
| 029 | sensor maps compare structurally | runtime |
| 030 | no sensor fusion from empty set | runtime |
| 031 | new rover is docked | runtime |
| 032 | nominal mission round trip | runtime |
| 033 | cannot drive before arming | runtime |
| 034 | repeated arm is not a second command | runtime |
| 035 | low power prevents arming | runtime |
| 036 | abort is terminal | runtime |
| 037 | abort stops driving motor | runtime |
| 038 | different robots have independent states | runtime |
| 039 | cannot skip to sampling | runtime |
| 040 | unknown transition is rejected | runtime |
| 041 | battery bounds are inclusive | runtime |
| 042 | battery underflow preserves old value | runtime |
| 043 | battery overflow preserves old value | runtime |
| 044 | invalid battery type is rejected | runtime |
| 045 | nan motor command is rejected | runtime |
| 046 | infinite motor command is rejected | runtime |
| 047 | motor bounds are inclusive | runtime |
| 048 | command queue capacity is enforced | runtime |
| 049 | immutable audit cannot be rewritten | runtime |
| 050 | immutable audit cannot be deleted | runtime |
| 051 | failed sample rolls back state and data | runtime |
| 052 | failed batch rolls back every slot | runtime |
| 053 | create then delete is undone | runtime |
| 054 | delete rollback restores existing record | runtime |
| 055 | repeated overwrites restore original | runtime |
| 056 | failed immutable append does not burn id | runtime |
| 057 | try is a nested savepoint | runtime |
| 058 | error kind and detail survive catch | runtime |
| 059 | short circuit and avoids invalid read | runtime |
| 060 | short circuit or avoids invalid read | runtime |
| 061 | negative list index reads last | runtime |
| 062 | out of range list index raises | runtime |
| 063 | fractional list index is not truncated | runtime |
| 064 | invalid range step is rejected | runtime |
| 065 | long command stream can stop early | runtime |
| 066 | queue elements obey declared type | runtime |
| 067 | map cannot silently accept list push | runtime |
| 068 | json preserves large command id | runtime |
| 069 | json overflow is rejected | runtime |
| 070 | untrusted json cannot forge unknown variants | runtime |
| 071 | empty string value is present | runtime |
| 072 | null value is distinct from missing key | runtime |
| 073 | null cannot be an accidental key | runtime |
| 074 | private storage keys are reserved | runtime |
| 075 | key and value views stay consistent | runtime |
| 076 | missing delete has no side effect | runtime |
| 077 | text command ids are not coerced | runtime |
| 078 | nested map update does not alias another value | runtime |
| 079 | unicode command payload roundtrips | runtime |
| 080 | integer clock difference stays exact | runtime |
| 081 | unbounded recursion is not a proof | verify_fail |
| 082 | guarded counter induction is provable | verify_ok |
| 083 | invalid state target is static error | check_fail |
| 084 | unsafe literal write cannot verify | verify_fail |
| 085 | dynamic state target cannot pass strict | verify_fail |
| 086 | replica count does not prove consensus | verify_fail |
| 087 | eventual cluster is not a strict safety proof | verify_fail |
| 088 | missing handler is rejected | check_fail |
| 089 | proven decreasing recursion terminates | verify_ok |
| 090 | typed boundary mismatch is reported | runtime |
| 091 | native encoder add promotion | runtime |
| 092 | native distance multiply promotion | runtime |
| 093 | native negative integer division | runtime |
| 094 | native zero divisor is catchable | runtime |
| 095 | native wide bit shift preserves encoder bits | runtime |
| 096 | committed telemetry survives process restart | process |
| 097 | failed handler leaves no persistent writes | process |
| 098 | failed transition leaves no persistent state | process |
| 099 | immutable audit survives process restart | process |
| 100 | record replay preserves failure classification | process |
