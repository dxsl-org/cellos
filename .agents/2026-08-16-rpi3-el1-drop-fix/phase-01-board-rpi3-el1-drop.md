---
phase: 1
title: "Split Board RPi3 EL2 Entry to EL1h"
status: pending
priority: P1
effort: "3h"
dependencies: []
tier: thinking
---

# Phase 1: Split Board RPi3 EL2 Entry to EL1h

> Required - deviation-log: Log every Decision / Deviation / Surprise here during implementation. Escalate if any source outside the files listed below becomes required.

## Overview

Change only the board-rpi3 EL2 boot path so real Pi 3 firmware EL2 entry drops to EL1h before `kmain`. Preserve the existing generic EL2 host path for QEMU virtualization and hypervisor lanes.

## Requirements

- Functional: under `#[cfg(feature = "board-rpi3")]`, when `_start` detects EL2, execute an EL2-to-EL1h handoff instead of the existing EL2-host `kmain` path.
- Functional: exact handoff sequence must set `HCR_EL2.RW=1`, `HCR_EL2.TGE=0`, preserve/respect CPTR_EL2 RES1 handling, set `CNTHCTL_EL2 = EL1PCTEN|EL1PCEN`, set `CNTVOFF_EL2=0`, set `SPSR_EL2=0x3c5`, set `ELR_EL2=.el1_entry`, then `ERET`.
- Functional: do not call `el2_mark_active` on board-rpi3 before `kmain`; `el2::is_el2()` must remain false after the drop.
- Non-functional: do not alter PTE `PXN`/`UXN` policy; Research 2's PTE/PXN alternative is rejected by hardware evidence because TGE makes S1E0R bypass effective EL1 MMU-on state.
- Non-functional: preserve DTB in `x19` across the handoff so `.el1_entry` still passes it to `kmain`.

## Architecture

Data flow:

1. Entry: firmware enters `_start` with `x0` DTB pointer; code saves it to `x19` before branching by `CurrentEL` (`hal/arch/arm/src/aarch64/boot.rs:32`, `hal/arch/arm/src/aarch64/boot.rs:37`).
2. Current board-rpi3 EL2 path wrongly stays at EL2 and marks `EL2_ACTIVE` (`hal/arch/arm/src/aarch64/boot.rs:43`, `hal/arch/arm/src/aarch64/boot.rs:84`).
3. New board-rpi3 path transforms EL2 firmware state into EL1h state: EL2 control regs configured -> `ELR_EL2=.el1_entry` -> `SPSR_EL2=0x3c5` -> `eret`.
4. Existing `.el1_entry` continues unchanged: enables `CPACR_EL1`, sets `SPSel=1`, stack, BSS, and calls `kmain` (`hal/arch/arm/src/aarch64/boot.rs:95`).
5. Because `EL2_ACTIVE` stays false, existing runtime components choose EL1 behavior: VBAR_EL1, `__switch_el1`, EL1/CNTP timer, and normal EL1 paging (`hal/arch/arm/src/aarch64/trap.rs:70`, `hal/arch/arm/src/aarch64/context.rs:63`, `hal/arch/arm/src/aarch64/timer.rs:41`, `hal/arch/arm/src/aarch64/paging.rs:217`).

Dependency graph:

`CurrentEL==2` -> `board-rpi3 cfg branch` -> `EL2 minimal handoff setup` -> `ERET to .el1_entry` -> existing EL1 boot -> existing EL1 runtime dispatch.

## Assumptions

- Claim: `SPSR_EL2=0x3c5` is the correct EL1h + DAIF-masked AArch64 state for this handoff.
  Confidence: high
  How to verify: disassemble boot object and confirm `msr spsr_el2, x?` immediate path; hardware boot must reach `.el1_entry` and later UART logs.
- Claim: CPTR_EL2 must not be blindly written to zero on Cortex-A53 if RES1 handling is required by this firmware/CPU state.
  Confidence: medium
  How to verify: implement using the same RES1-safe convention already documented in vCPU code before build; if direct code precedent is not reusable, preserve current working CPTR behavior and document exact value.

## Related Files

- Modify: `hal/arch/arm/src/aarch64/boot.rs`
- Modify only if helper/comment extraction is justified: `hal/arch/arm/src/aarch64/el2.rs`
- Read-only: `hal/arch/arm/src/aarch64/trap.rs`
- Read-only: `hal/arch/arm/src/aarch64/context.rs`
- Read-only: `hal/arch/arm/src/aarch64/timer.rs`
- Read-only: `hal/arch/arm/src/aarch64/paging.rs`

## Implementation Steps

1. In `boot.rs`, split `.el2_init` by compile-time `board-rpi3` gating so board-rpi3 enters a new EL2-drop path and non-board builds keep the current EL2-host path.
2. In the board-rpi3 EL2-drop path, set `HCR_EL2` to `RW` only; explicitly leave `TGE=0`.
3. Program CPTR_EL2 with safe RES1 handling. Do not broaden this into lazy SIMD policy; the goal is simply not trapping EL1/EL0 FP/SIMD unexpectedly during boot.
4. Program `CNTHCTL_EL2` with `EL1PCTEN|EL1PCEN` so EL1 can access physical counter/timer registers, then set `CNTVOFF_EL2=0`.
5. Set `ELR_EL2` to `.el1_entry`, set `SPSR_EL2=0x3c5`, `isb`, then `eret`.
6. Do not clear BSS or call `kmain` in the board-rpi3 EL2 path; `.el1_entry` owns stack setup, BSS clearing, and `kmain`.
7. Do not call `el2_mark_active` in the board-rpi3 drop path.
8. Keep generic `.el2_init` behavior intact for non-board builds, including `el2_mark_active` and EL2-host `kmain`.

## Success Criteria

- [ ] `board-rpi3` disassembly shows `msr hcr_el2` value has bit 31 set and bit 27 clear on the board-rpi3 EL2 handoff path.
- [ ] `board-rpi3` disassembly shows `msr cnthctl_el2`, `msr cntvoff_el2`, `msr spsr_el2`, `msr elr_el2`, and `eret` before `.el1_entry`.
- [ ] No `bl el2_mark_active` appears on the board-rpi3 EL2-drop path.
- [ ] Non-board AArch64 EL2 path still contains `bl el2_mark_active` before `kmain`.

## Test Matrix

- Unit/static: `cargo build --release --features board-rpi3 -p vicell-kernel --target aarch64-unknown-none-softfloat`.
- Static binary: `aarch64-linux-gnu-objdump -d target/aarch64-unknown-none-softfloat/release/vicell-kernel` and inspect `_start`, `.el2_init`, `.el1_entry`.
- Integration: `.\run-rpi3.ps1 -SdImage` or direct kernel QEMU raspi3b smoke, expected to follow the EL1 path when QEMU starts at EL2/EL1 depending model behavior.
- Generic regression: build/run the non-board AArch64 virtualization lane that depends on `EL2_ACTIVE` and stage-2/vCPU helpers.

## Backwards Compatibility

Board-rpi3 runtime changes from EL2-host to EL1-kernel, but this is the compatibility target for current real hardware. Generic non-board EL2 behavior remains unchanged. Cell ABI and page-table permissions remain unchanged.

## Risk Assessment

- Medium likelihood x High impact: compile-time gating accidentally changes generic EL2 host boot. Mitigation: inspect disassembly for both board and non-board builds; keep non-board `.el2_init` body byte-for-byte equivalent where possible.
- Medium likelihood x High impact: wrong EL2 handoff register setup hangs before UART. Mitigation: keep the sequence minimal, preserve DAIF masked in `SPSR_EL2=0x3c5`, and use disassembly before hardware.
- Low likelihood x Medium impact: `x19` DTB preservation is broken across `eret`. Mitigation: do not clobber x19 in the handoff block; `.el1_entry` already consumes x19.
- Rollback: revert `boot.rs`/optional `el2.rs` hunks and rebuild previous diagnostic image. Irreversible part: none in persistent storage; failed boots only cost board time.

## Security Considerations

Do not weaken W^X/PXN/UXN policy. Do not expose the EL choice as a runtime knob. The generic EL2 virtualization path remains privileged and unavailable unless the build/runtime actually marks `EL2_ACTIVE`.

## Deviation Log

None.
