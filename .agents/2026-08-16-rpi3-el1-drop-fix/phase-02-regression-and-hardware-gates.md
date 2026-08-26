---
phase: 2
title: "Run Regression and Hardware Gates"
status: pending
priority: P1
effort: "2h"
dependencies: [1]
tier: medium
---

# Phase 2: Run Regression and Hardware Gates

## Overview

Prove the boot split with build, disassembly, QEMU, generic EL2 regression, and real Pi 3 UART evidence while keeping the temporary TGE probe for the first fixed hardware boot.

## Requirements

- Functional: preserve current `trap.rs` temporary `par_tge0` probe for the first fixed boot (`hal/arch/arm/src/aarch64/trap.rs:90`, `hal/arch/arm/src/aarch64/trap.rs:189`).
- Functional: capture logs showing the fixed boot no longer dies at the initial EL0 instruction fetch.
- Non-functional: no cleanup or architecture simplification until hardware evidence is saved.

## Architecture

Data enters as built `vicell-kernel` plus SD image. It is transformed by static disassembly and runtime boot tests into evidence artifacts. Exits are raw UART logs, hashes, and a short diagnosis note under `.agents/debug/`.

## Assumptions

- Claim: QEMU raspi3b is useful for smoke but not decisive for the TGE hardware failure.
  Confidence: high
  How to verify: keep QEMU as pass/fail for gross boot regressions only; hardware UART is the acceptance gate.

## Related Files

- Read-only: `run-rpi3.ps1`
- Read-only: `gen_disk_rpi3.ps1`
- Create: `.agents/debug/<timestamp>-rpi3-el1-drop-*.raw`
- Create: `.agents/debug/<timestamp>-rpi3-el1-drop-report.md`

## Implementation Steps

1. Run the board-rpi3 release build: `cargo build --release --features board-rpi3 -p vicell-kernel --target aarch64-unknown-none-softfloat`.
2. Disassemble the release kernel and verify the Phase 1 success criteria before flashing.
3. Run `.\run-rpi3.ps1` and/or `.\run-rpi3.ps1 -SdImage` as a gross regression gate.
4. Run the generic non-board AArch64 EL2/virtualization regression lane used by the repo; verify `EL2_ACTIVE` still becomes true outside board-rpi3.
5. Generate SD image with `.\gen_disk_rpi3.ps1`.
6. Flash to the confirmed removable drive using the already established safe Windows flash process; require explicit physical drive confirmation.
7. Boot real RPi3 and capture UART for at least 30 seconds.
8. Save raw UART and SHA-256 under `.agents/debug/`, then summarize the decisive lines.

## Success Criteria

- [ ] Board-rpi3 release build passes.
- [ ] Disassembly confirms board-rpi3 EL2 path drops to `.el1_entry` and does not call `el2_mark_active`.
- [ ] QEMU raspi3b smoke emits expected serial output or reaches the same known host-gated limit without new earlier failure.
- [ ] Generic non-board EL2 regression still boots/executes with `EL2_ACTIVE=true`.
- [ ] Real RPi3 UART no longer shows the prior TGE-driven identity `AT S1E0R` failure as the terminal condition.
- [ ] Evidence files include raw log path and SHA-256.

## Test Matrix

- Build: board-rpi3 release cargo build.
- Static: objdump path audit for HCR/CNTHCTL/CNTVOFF/SPSR/ELR/ERET and absence/presence of `el2_mark_active` by cfg.
- Integration: QEMU raspi3b smoke.
- Regression: generic AArch64 EL2 virtualization lane.
- E2E: real RPi3 SD boot and UART capture.

## Backwards Compatibility

No user-facing ABI change. If generic EL2 regression fails, stop before Phase 3 and treat Phase 1 as incorrectly scoped.

## Risk Assessment

- Medium likelihood x Medium impact: QEMU result differs from hardware. Mitigation: hardware is authoritative; QEMU only catches gross regressions.
- Medium likelihood x High impact: generic EL2 lane regresses silently. Mitigation: make generic regression mandatory before cleanup.
- Medium likelihood x Medium impact: flash writes wrong target. Mitigation: retain explicit PHYSICALDRIVE confirmation; never auto-flash an unconfirmed device.
- Rollback: use the previous known diagnostic image or revert Phase 1 source hunks. Irreversible part: none beyond SD image overwrite after explicit confirmation.

## Security Considerations

Keep raw logs free of secrets; UART should contain kernel boot diagnostics only.

## Deviation Log

None.
