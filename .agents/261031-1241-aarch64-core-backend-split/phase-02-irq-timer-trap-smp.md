# Phase 02 — IRQ, timer, trap, SMP  (checkpoint)

**Status:** implemented 2026-11-02, uncommitted; gate PASS ([reports/phase-02-gate.md](reports/phase-02-gate.md)). Waiting for checkpoint approval; Phase 03 not started.

## Context Links
[plan](plan.md) · [scout](scout-report.md) · ADR-0029 §6

## Overview
Move the interrupt controller, timer, IRQ dispatch and SGI/secondary-start choices behind the backend.

## Key Insights
- RPi3 IRQ uses BCM2836 local + BCM2835 legacy, with no SGI (single hart, `smp.rs:687-690`). RPi4 and virt share GICv2 with different bases.
- QEMU-only PL061/VirtIO dispatch in `trap.rs` is a `virt` backend concern.

## Requirements
- `ArmIrqController` (init, enable, claim, complete, `send_sgi -> Option`), `ArmTimer` (init, reset, routing).
- `gic.rs` becomes base-parameterised from the profile and is used by the `virt` and `bcm2711` backends.
- BCM local/legacy/systimer code moves into `backend/bcm2837`.
- `trap.rs`, `aarch64.rs`, `timer.rs` and `task/smp.rs` contain no `board-rpi*` cfg.
- Secondary-core start is the backend's choice (PSCI vs none).

## Architecture
Static dispatch through `Board` (see the plan's key design decisions). Only `backend/mod.rs` may contain `cfg(feature = "board-rpi*")` after this phase, for the files it covers.

## Related Code Files
`hal/arch/arm/src/aarch64{.rs,/gic.rs,/timer.rs,/trap.rs}`, `kernel/src/task/smp.rs`, `backend/*`.

## Implementation Steps
1. Define IRQ and timer traits
2. Parameterise GIC by profile
3. Move BCM IRQ and systimer into bcm2837 backend
4. Move trap dispatch and SMP start
5. Run preservation gate with SMP=2

## Todo List
- [x] Define IRQ and timer traits (`ArmIrqController`, `ArmTimer`, `SecondaryStart`)
- [x] Parameterise GIC by profile (`GicV2Layout`)
- [x] Move BCM IRQ and systimer into bcm2837 backend (with `uart_bcm_mini`, Q3)
- [x] Move trap dispatch and SMP start (plus `syscall.rs` `WaitIrq` USB unmask, Q2)
- [x] Run preservation gate with SMP=2

Deviations and file-set amendments are in the gate report §1 and §6. Summary:
- `aarch64.rs` keeps the `monitor` module gate; it moves with `monitor.rs` in Phase 04.
- Path-only edits were made at `main.rs:792,1688-1733` and `hypervisor/registry.rs:832,1009,1032`.
- `docs/generated/mutable-globals.md` was regenerated (Q7).
- The RPi3 tick criterion is restated by Q10(a); the tick proxies are recorded in the gate report.

## Success Criteria
Checkpoint:
- The gate passes, including `QEMU_SMP=2` test-hooks.
- The RPi3 `raspi3b` boot reaches the prompt and the timer tick log matches the baseline cadence.
- The four files are free of `board-rpi*`.

## Risk Assessment
- The IRQ hot path regresses if the trait adds indirection. Static dispatch plus `.text` diff guard against that.
- RPi3 single-hart semantics must be kept (no SGI).
- Rollback: revert.

## Security Considerations
IRQ routing determines which Cell receives device interrupts. The resource-registry mapping is untouched in this phase.

## Next Steps
Phase 04 after phase 03 lands.
