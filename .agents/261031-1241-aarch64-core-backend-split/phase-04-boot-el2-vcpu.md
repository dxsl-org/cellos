# Phase 04 — Boot/DTB, board selection, EL2/vCPU monitor  (checkpoint)

**Status:** implemented 2026-11-02, uncommitted. Gate rows PASS; the hypervisor lane fails identically at the pre-plan baseline, a guest-initramfs artefact ([reports/phase-04-gate.md](reports/phase-04-gate.md) §3.1). Awaiting checkpoint approval.

## Context Links
[plan](plan.md) · [scout](scout-report.md) · ADR-0029 §6

## Overview
Finish the split: descriptor selection, DTB and memory policy, the boot asm secondary mode, and the RPi3 EL2 monitor and vCPU extension state.

## Key Insights
- `board.rs:108-203` collapses to `Board::DESCRIPTOR`.
- `vcpu.rs` RPi3 state becomes `Board::VcpuExt`. The `AArch64Vcpu` layout per build must be identical (assert `size_of` and offsets against the baseline).
- This phase conflicts with the parked RPi3 EL1-drop plan on `boot.rs`. Do not run them together.

## Requirements
- `ArmPlatform::DESCRIPTOR`, `dtb_policy()` (Optional/Conservative/Required), `secondary_mode()`, `type VcpuExt`, `el2_monitor: Option<…>`.
- Remove the `board-rpi*` cfg from `board.rs`, `boot.rs`, `cpu_features.rs`, `el2.rs`, `paging.rs`, `stage2_regs.rs`, `vcpu.rs`, `hypervisor/registry.rs`, the remaining `main.rs` sites and `boot.rs` asm (via a `const` from the backend).
- End state: `rg 'feature = "board-rpi'` in `hal/arch/arm/src` and `kernel/src` hits only `backend/mod.rs` and Cargo feature wiring.
- Update `docs/system-architecture.md` (aarch64 section) and the changelog.

## Architecture
Static dispatch through `Board` (see the plan's key design decisions). Only `backend/mod.rs` may contain `cfg(feature = "board-rpi*")` after this phase, for the files it covers.

## Related Code Files
`kernel/src/{board.rs,boot.rs,cpu_features.rs,main.rs,hypervisor/registry.rs}`, `hal/arch/arm/src/aarch64/{boot.rs,el2.rs,paging.rs,stage2_regs.rs,vcpu.rs}`, `backend/*`, docs.

## Implementation Steps
1. Move descriptor and DTB policy
2. Move boot asm secondary mode
3. Introduce VcpuExt with layout assertions
4. Move EL2 monitor and stage-2 routing
5. Final rg audit, docs and changelog

## Todo List
- [x] Move descriptor and DTB policy (`board.rs` via `SOC`; `DtbPolicy` Optional/Conservative/Required)
- [x] Move boot asm secondary mode (`PI_MONITOR_BOOT`, `SECONDARY_START` consts)
- [x] Introduce VcpuExt with layout assertions (`ArmVcpuExt`; `AArch64Vcpu` 536/536 and 544/1632 asserted per build; `VcpuRegs` mode via `REGS_MAX_MODE`, Q1)
- [x] Move EL2 monitor and stage-2 routing (`monitor.rs` → `backend/monitor.rs`; `ArmEl2Monitor`)
- [x] Final rg audit, docs and changelog (`rg` hits only `backend/mod.rs` and the `main.rs:13` conflict `compile_error!`; `docs/system-architecture.md` updated; CHANGELOG bullet proposed, not applied)

Deviations are in the gate report §5.

## Success Criteria
Checkpoint:
- Full gate passes, including `run-hypervisor-arm.ps1` (virt EL2/vGIC).
- `AArch64Vcpu` layout assertions pass for each variant.
- The final `rg` shows hits only in `backend/mod.rs`.
- The docs are updated.

## Risk Assessment
- The EL2 monitor path is the hardest to verify. Its RPi3 runtime evidence is QEMU `raspi3b` only (A1).
- vCPU layout drift would break guest state. The assertions guard it.
- Rollback: revert the phase.

## Security Considerations
Stage-2 and monitor gateway code enforce guest isolation. Move only, with no logic change. Review the diff for any altered barrier or TLB sequence.

## Next Steps
Group 2 (SystemReady) and group 3 (Apple) backends can be designed against these traits when scheduled. That requires a new ADR-0022 amendment.
