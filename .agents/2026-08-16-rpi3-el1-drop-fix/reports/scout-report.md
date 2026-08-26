# Scout Report: RPi3 EL1 Drop Fix

## Verified Code Paths

- `hal/arch/arm/src/aarch64/boot.rs:37` reads `CurrentEL`; `boot.rs:40` branches EL2 to `.el2_init`; `boot.rs:41` branches otherwise to `.el1_entry`.
- `hal/arch/arm/src/aarch64/boot.rs:43` current `.el2_init` begins; `boot.rs:44` documents `RW|TGE`; `boot.rs:85` calls `el2_mark_active`; `boot.rs:88` calls `kmain`.
- `hal/arch/arm/src/aarch64/boot.rs:95` current `.el1_entry` begins; `boot.rs:99` writes `cpacr_el1`; `boot.rs:108` writes `spsel`; `boot.rs:130` calls `kmain`.
- `hal/arch/arm/src/aarch64/el2.rs:37` implements `is_el2`; `el2.rs:46` implements `el2_mark_active`; `el2.rs:86` implements `el2_mmu_init`.
- `hal/arch/arm/src/aarch64/trap.rs:70` selects VBAR_EL2 only when `is_el2`; otherwise VBAR_EL1.
- `hal/arch/arm/src/aarch64/context.rs:63` selects `__switch_el2` only when `is_el2`; otherwise `__switch_el1`.
- `hal/arch/arm/src/aarch64/timer.rs:41` board-rpi3 path arms BCM2835 system timer and CNTP; generic non-board path checks `is_el2` at `timer.rs:74`.
- `hal/arch/arm/src/aarch64/paging.rs:217` calls `el2_mmu_init` only when `is_el2`; otherwise stays on EL1 activation.
- `hal/arch/arm/src/aarch64/stage2_regs.rs:3` and `hal/arch/arm/src/aarch64/vcpu.rs:204` require true EL2, so generic virtualization must not be disabled globally.

## Research Inputs Applied

- Research 1 accepted: board-rpi3 should drop EL2->EL1 before `kmain`; existing `is_el2()==false` branches align with that target.
- Research 2 PTE/PXN recommendation rejected: real hardware showed TGE changes S1E0R from identity to correct PA; when effective `SCTLR_EL1.M=0`, PTE bits are not the root-cause lever.

## Constraints

- Source worktree is dirty; implementation must not revert unrelated changes.
- Temporary `trap.rs` `par_tge0` diagnostic should stay for the first fixed hardware boot and be cleaned only after hardware pass.
- `.claude/scripts/set-active-plan.cjs` is absent; active-plan sync cannot be performed.
