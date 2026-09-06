---
phase: 4
title: "Run Production Regression and Close Documentation"
status: complete
priority: P1
effort: "3h"
dependencies: [3]
tier: medium
---

# Phase 4: Run Production Regression and Close Documentation

> **Required — deviation-log:** Log every Decision / Deviation / Surprise below when it occurs. Verification failures are investigated at the source; never weaken the oracle or add handler special cases.

## Overview

Run the complete focused proof in order, then rebuild and boot the normal x86 image to prove test hooks and the entry cutover did not regress production. Close only documentation triggered by the new test lane and corrected dispatch behavior.

## Requirements

- Functional: CPL-aware host policy, target compile, full GPR/DF/alignment actual-entry proof, production boot, and serial-input regression all pass.
- Non-functional: production excludes probe/recovery/debug-exit code; 0xff returns without EOI; no excluded subsystem changes; final deviations and baseline differences are recorded.
- Documentation: add the dedicated test command to the q35 guide and summarize user-fault retirement, #PF/#GP separation, spurious-vector handling, and EOI behavior under CHANGELOG Unreleased.
- Late gate: do not commit or close the per-vector change until the mandatory
  `+pku` real-CPL3 oracle passes and production symbol isolation is rechecked.
  The former CPL0-only status is historical evidence, not acceptance.

## Architecture

Verification proceeds from cheapest/purest to most integrated: host policy → bare-metal link → dedicated actual entry → production ISO boot → production UART input. The `x86-idt-cpl3-test` ISO and production ISO use separate target/root/output paths, so the latter is necessarily rebuilt and cannot pass using the probe image; generic `test-hooks` remains non-terminal.

## Assumptions

- **Claim:** required local QEMU/xorriso prerequisites are available in the implementation environment.
  **Confidence:** medium
  **How to verify:** the dedicated build/runner and production ISO scripts fail closed with prerequisite messages; record an infrastructure blocker rather than marking success.

## Related Files / Ownership

| File | Action | Owner | Condition |
|---|---|---|---|
| `boards/qemu/q35-x86_64/README.md` | Document new test lane | Phase 4 | docs trigger fired |
| `CHANGELOG.md` | Add Unreleased behavior/test note | Phase 4 | existing file confirmed |
| `README.md` | Refresh x86 status and link board commands | Phase 4 | shipped QEMU evidence |
| `docs/system-architecture.md` | Record entry ABI and routing architecture | Phase 4 | architecture changed |
| `docs/project-roadmap.md` | Mark the QEMU-scoped lane regression-only | Phase 4 | roadmap status changed |
| `docs/roadmap/completed-history.md` | Add the completed hardening slice | Phase 4 | completion ledger |
| `docs/project-changelog.md` | Record detailed shipped behavior/evidence | Phase 4 | living changelog |
| implementation files from Phases 1–3 | Read/review only | Phase owners | change only to fix a proved defect |

## Implementation Steps

1. Review generated assembly for 256 labels/table entries and the exact error set; inspect linked addresses rather than trusting generator source alone.
2. Run the focused host policy command and require saved-RPL0/RPL3 cases plus 0xff spurious/no-EOI.
3. Run `cargo check -p cellos-kernel --target x86_64-unknown-none` and compare with Phase1 baseline.
4. Build the isolated test image, then run `objdump -d --disassemble=x86_64_idt_common target/x86-idt-test/x86_64-unknown-none/release/cellos-kernel`; require dynamic `and` immediately before the dispatch call with no intervening stack change.
5. Run `BOOT_WINDOW=90 bash scripts/qemu-x86_64-idt-test.sh`; require underlying status33, one exact CPL0 marker, one exact real-CPL3 marker, and no forbidden output.
6. Rebuild without test-hooks: `cargo build --release -p cellos-kernel --target x86_64-unknown-none -Z build-std=core,alloc && bash scripts/x86/make-iso-ci.sh && BOOT_WINDOW=90 bash scripts/qemu-x86_64-test.sh`.
7. Against that production ISO, run `cargo test --manifest-path tests/integration/Cargo.toml --test x86_64-boot -- x86_echo_command` to cover timer-driven scheduling and UART IRQ I/O.
8. Verify production and generic `test-hooks` contain no dedicated fixture symbols/namespaces/markers; confirm 0x80 DPL3, 0xff no-EOI return, and the repaired IDT/SYSCALL/fresh-IRET GS/PKRU contracts.
9. Update board guide prerequisites/commands/oracle and add a concise Unreleased entry. Keep statuses pending until criteria are actually met; append live deviations during implementation.

## Test Scenario Matrix

| Priority | Lane | Pass oracle |
|---|---|---|
| critical | CPL0 actual entry | status33 lane retains exact marker; two records/post-iret captures match 15 GPRs and DF states |
| critical | CPL3 transition | two real tasks prove fresh IRET, INT80, timer switch, suspended-SYSCALL resume, GS balance, and distinct PKRU |
| critical | call alignment | shim arithmetic and linked disassembly independently prove pre-call RSP%16=0 |
| critical | CPL/spurious policy | Ring3 attributable faults retire Cell; kernel/non-attributable faults fatal; 0xff returns/no-EOI |
| critical | production boot | normal ISO reaches `Cellos >`, no panic/fault |
| high | UART regression | `echo x86-ok` round-trips through COM1 |
| high | feature isolation | production and generic `test-hooks` exclude dedicated fixture symbols/namespaces/markers and terminal hook |

## Original CPL0 Success Criteria

- [x] All five commands in `plan.md` pass; actual-entry evidence includes exact GPR, DF, alignment, and timer fields.
- [x] Pure tests cover RPL0/RPL3 attribution, NMI/#DF/#MC fatality, and 0xff no-callback/no-EOI.
- [x] Test and production ISO paths are distinct, and production is rebuilt after probe proof.
- [x] Production reaches shell/UART echo and contains no probe recovery/debug-exit code.
- [x] Board docs and CHANGELOG Unreleased describe only shipped behavior/commands.
- [x] The original inventory excluded TSS/IST, VMM guest, emulator-version, and unrelated changes; the late safety gate made logged IDT/SYSCALL/fresh-exit corrections.

## Late-Gate Success Criteria

- [x] Dedicated `+pku` real-CPL3 oracle passes without forbidden output.
- [x] Production is rebuilt and passes boot/UART regression.
- [x] Production and generic `test-hooks` contain no dedicated CPL3 fixture,
  scheduler-hook, marker, or HAL debug-exit code.

## Final Verification Evidence (2026-09-02)

- `[PASS]` Formatting, 9/9 HAL host tests, 89/89 kernel host tests, production
  and generic-`test-hooks` x86 check/Clippy gates, deterministic generator
  checks, and production/dedicated linked contracts all passed.
- `[PASS]` The dedicated runner exited 0 while enforcing QEMU status 33,
  exactly one
  `X86-IDT-SELFTEST: PASS bp=3 gp=13/ec=fffc gprs=15 df=ok align=ok timer=32`
  marker, exactly one
  `X86-IDT-CPL3: PASS fresh=ok int80=ok timer=32 switch=syscall-resume gs=kernel/user pkru=0/55555550/55555544`
  marker, two pre-scheduler timer wakeups, one scheduler initialization, and no
  forbidden output.
- `[PASS]` The isolated feature/dependency graph proved generic `test-hooks`
  does not select the terminal fixture or HAL `qemu-exit`; only
  `x86-idt-cpl3-test` does.
- `[PASS]` The fresh production ISO packaged the verified release ELF, excluded
  all 18 fixture symbols, six namespaces, and both markers, reached the shell,
  and passed all 7 x86 boot integration tests.
- `[PASS]` Final verification, final CPL3 review, and final oracle stress review
  all returned PASS with no findings.

## Historical Completion Evidence (superseded by reopened safety gate)

- The complete acceptance sequence passed: 9 HAL host tests, 89 kernel host
  tests, x86-none check and warning-denying Clippy, deterministic generated-stub
  validation, linked release disassembly/table inspection, the dedicated QEMU
  lane, a fresh production ISO boot, and all 7 x86 boot integration tests.
- The dedicated image exited with debug status 33 and exactly one
  `X86-IDT-SELFTEST: PASS bp=3 gp=13/ec=fffc gprs=15 df=ok align=ok timer=32`
  marker. The production image was rebuilt afterward, reached the shell without
  panic/fault/reset output, passed the UART integration coverage, and contained
  no probe/debug-exit symbols or marker.
- `git diff --check` passed against baseline `0136c58e`; the final inventory
  found no excluded subsystem change. Generated production ISO/kernel artifacts
  were restored after validation.
- Final verification and the independent entry-boundary stress review both
  returned PASS with no findings.

## Security Considerations

Inspect the production binary, not only cfg source, for recovery/capture/debug-exit residue. Treat residue as release-blocking. Confirm pure CPL policy prevents a Cell exception from halting the host while NMI/#DF/#MC cannot be misattributed, and confirm 0xff never EOIs.

## Risk Notes

The production script previously tolerated QEMU timeout internally before checking serial. Its shell-prompt oracle is still authoritative for this regression; do not retrofit debug-exit into that runner or conflate the two lanes.

## Deviation Log

- **Surprise (2026-09-02):** The first strict actual-entry run rejected the
  image because the bootstrap tail-jumped into compiled Rust with the wrong
  SysV stack phase. The causal correction reserves one zeroed 8-byte synthetic
  bottom-frame slot after 16-byte alignment; the tail-jump and never-return
  contract are unchanged. The corrected lane reached the #BP, #GP, and timer
  oracle and exited with status 33.
- **Resolution (2026-09-02):** The final Cargo.lock audit found only the
  expected `qemu-exit` dependency edge for `hal-x86`; the package already
  existed in the workspace lockfile. No unrelated baseline delta was hidden.
- **Resolved blocker (2026-09-02):** Phase 4 was reopened because its first
  dedicated evidence covered CPL0 only. The final `+pku` two-task real-CPL3
  oracle passed with status 33 and exact marker counts, and the fresh production
  regression passed.
- **Resolved source blocker (2026-09-02):** Final review found the Ring-3
  oracle hijacked all x86 `test-hooks` images. The terminal fixture and HAL
  debug-exit edge are now gated by `x86-idt-cpl3-test`, which depends on
  `test-hooks` but is enabled only by the isolated IDT build script. Final
  inspection found generic `test-hooks` and production fixture-free; both final
  reviewers returned PASS.
