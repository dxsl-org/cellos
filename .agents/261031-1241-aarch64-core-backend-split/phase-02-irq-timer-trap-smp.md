# Phase 02 — IRQ, timer, trap, SMP  (checkpoint)

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
- [ ] Define IRQ and timer traits
- [ ] Parameterise GIC by profile
- [ ] Move BCM IRQ and systimer into bcm2837 backend
- [ ] Move trap dispatch and SMP start
- [ ] Run preservation gate with SMP=2

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
