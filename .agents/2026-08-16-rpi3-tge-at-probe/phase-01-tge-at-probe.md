---
phase: 1
title: "Add One-Variable TGE AT Probe"
status: pending
priority: P1
effort: "2h"
dependencies: []
tier: thinking
---

# Phase 1: Add One-Variable TGE AT Probe

> Required - deviation-log: Log every Decision / Deviation / Surprise in this file during implementation. On divergence, choose the smallest reversible option, log four lines, and continue unless the result would change architecture or require another source file.

## Overview

Add one temporary diagnostic to the existing board-rpi3 uncategorized EL2 fault probe: snapshot `HCR_EL2`, clear bit 27 (`TGE`), run a second `AT S1E0R(pc)`, read `PAR_EL1`, restore original `HCR_EL2`, then log the added result beside the existing baseline.

## Requirements

- Functional: inside `probe_uncategorized_el2_fault`, capture `hcr_saved`, compute `hcr_without_tge = hcr_saved & !(1 << 27)`, write it to `HCR_EL2`, `ISB`, run `AT S1E0R` on `frame.elr_el1`, `ISB`, read `PAR_EL1`, restore `hcr_saved`, `ISB`, and print both saved HCR and the new PAR value.
- Functional: preserve the existing baseline `par_s1e0r` and `par_s1e2r` logging.
- Non-functional: keep the probe `#[cfg(feature = "board-rpi3")]` and only active for the existing `ec == 0` path.
- Non-functional: do not change page-table descriptor bits, EL0 entry, EL2 boot HCR policy, UART setup, flash scripts, or firmware config.

## Architecture

OBSERVED data flow:

1. Lower-EL sync trap reaches `vi_aarch64_trap_handler`; the board-rpi3 probe runs only when `ec == 0` (`hal/arch/arm/src/aarch64/trap.rs:179`, `hal/arch/arm/src/aarch64/trap.rs:183`).
2. Probe reads fault PC from `frame.elr_el1` (`hal/arch/arm/src/aarch64/trap.rs:111`).
3. Current baseline samples `AT S1E0R(pc)` to `PAR_EL1` and `AT S1E2R(pc)` to `PAR_EL1` (`hal/arch/arm/src/aarch64/trap.rs:116`, `hal/arch/arm/src/aarch64/trap.rs:126`).
4. Planned temporary branch inserts one extra transform: `HCR_EL2.saved -> clear TGE -> AT S1E0R(pc) -> PAR_EL1 -> restore saved HCR`.
5. UART exits as `FS*` diagnostic text through `uart_bcm_mini` (`hal/arch/arm/src/aarch64/trap.rs:137`, `hal/arch/arm/src/aarch64/trap.rs:151`).

Dependency graph:

`trap.rs probe entry` -> `baseline AT logs` -> `temporary HCR.TGE-cleared AT log` -> `restore HCR` -> `existing fault handling/logging`.

## Assumptions

- Claim: Real RPi3 firmware/CPU behavior is required to decide this, because QEMU may not model the same non-VHE EL2/TGE interaction.
  Confidence: medium
  How to verify: compare real board UART output against `.\run-rpi3.ps1 -SdImage` output after the same temporary probe.
- Claim: The current SD/serial process remains available.
  Confidence: high
  How to verify: user confirms F: card visibility and COM4 capture before boot.

## Related Files

- Modify: `hal/arch/arm/src/aarch64/trap.rs`
- Read-only reference: `hal/arch/arm/src/aarch64/el2.rs`
- Read-only reference: `hal/arch/arm/src/aarch64/paging.rs`
- Read-only reference: `run-rpi3.ps1`
- Read-only reference: `gen_disk_rpi3.ps1`

## Implementation Steps

1. In `probe_uncategorized_el2_fault`, add a new `par_s1e0r_tge0: u64` local next to the existing `par_s1e0r` and `par_s1e2r` locals.
2. In one inline assembly block after the existing baseline `AT` samples, read `HCR_EL2` to a saved register/output, clear bit 27 into a temporary value, write `HCR_EL2`, issue `ISB`, run `AT S1E0R, pc`, issue `ISB`, read `PAR_EL1` into `par_s1e0r_tge0`, restore saved `HCR_EL2`, and issue final `ISB`.
3. Mark the assembly block as privileged EL2-only with a contract comment; keep it inside the already EL2-gated probe path.
4. Add the shortest possible log fields, for example `hcr0` or `hcr_saved` and `par_tge0`, without removing `FS0`, `FS1`, or `FS2` existing output.
5. Build with the board-rpi3 lane from the existing script contract: `cargo build --release --features board-rpi3 -p vicell-kernel --target aarch64-unknown-none-softfloat` (`gen_disk_rpi3.ps1:37`) or `.\run-rpi3.ps1` which runs that build (`run-rpi3.ps1:49`).
6. Generate SD image with `.\gen_disk_rpi3.ps1`; it packages `kernel8.img` plus firmware files (`gen_disk_rpi3.ps1:1`, `gen_disk_rpi3.ps1:40`, `gen_disk_rpi3.ps1:179`).
7. Flash and boot on real RPi3 through the existing manual lane, capture UART, and compare baseline `par` against new `par_tge0`.
8. Immediately revert this temporary probe after capturing the result; any architecture fix must be a separate plan.

## Success Criteria

- [ ] Build succeeds for `vicell-kernel` with `--features board-rpi3`.
- [ ] UART still reaches the existing `FS0`, `FS1`, `FS2`, and `T00` markers.
- [ ] UART includes one new `AT S1E0R` result with `HCR_EL2.TGE` temporarily cleared, and HCR is restored before normal fault handling continues.
- [ ] Captured log is saved under `.agents/debug/` with raw output and SHA-256.
- [ ] No source file other than `hal/arch/arm/src/aarch64/trap.rs` changes for the diagnostic.

## Test Matrix

- Unit/static: `cargo build --release --features board-rpi3 -p vicell-kernel --target aarch64-unknown-none-softfloat`.
- Integration: optional `.\run-rpi3.ps1 -SdImage` smoke to ensure image still boots far enough to emit serial output; result is advisory only for this EL2/TGE question.
- E2E hardware: real RPi3 boot with captured UART output; this is the decisive acceptance gate.

## Backwards Compatibility

No committed compatibility change is intended. The temporary diagnostic must be reverted after evidence capture, so downstream Cell ABI, syscall routing, page permissions, SD image layout, and firmware config remain unchanged.

## Risk Assessment

- High likelihood x Medium impact: clearing `TGE` even briefly could change exception routing if an interrupt/exception occurs inside the probe window. Mitigation: keep window to one `AT` sequence, preserve DAIF state, restore HCR before all normal handler work, and do not call into Rust while HCR is modified.
- Medium likelihood x High impact: failure to restore HCR would invalidate later logs and could hang boot. Mitigation: use one inline assembly block with restore in the same block before any branch or function call.
- Low likelihood x Medium impact: QEMU passes while real hardware differs. Mitigation: hardware log is the only success gate.
- Rollback: revert the single `trap.rs` hunk and rebuild/flash the prior known image. Irreversible part: board time spent and any already-captured misleading log; no persistent on-board state changes are expected.

## Security Considerations

The diagnostic temporarily changes EL2 trap control on a fault path only and must not ship. Do not expose this as a runtime switch, syscall, config flag, or general debug feature.

## Deviation Log

None.
