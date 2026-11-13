---
phase: 14
title: "Kernel arch seam and boot split"
status: in-progress
priority: P2
effort: "10–14d"
dependencies: [9]
tier: thinking
checkpoint: false
---

# Phase 14: Kernel `arch/` seam and boot split

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs.

## Context Links

[Review H-9, Medium IPC/memory/hardware](../reports/architecture-review-261110.md) · `hal/core/src/lib.rs` · `hal/traits/` · `kernel/src/main.rs` · `kernel/src/board.rs` · `kernel/src/platform.rs` · `kernel/src/task.rs` (`yield_cpu`) · `kernel/src/memory/paging.rs` · `kernel/src/task/user_copy/sv39_probe.rs` · [Spec 04](../../docs/specs/04-hardware.md) · [ADR-0029](../../docs/decisions/0029-platform-image-families-and-runtime-detection.md) · owner decision 4

## Overview

Give the kernel one place where architecture differences live, so x86 work stops paying a cfg tax across 92 files and parked architectures are quarantined rather than interleaved.

## Key Insights

- Reviewer-cited: `Timer`, `InterruptController`, `BootController` have zero implementors; `ViHypervisor` is stub-only; kernel names `hal::aarch64::` 103× and `hal::x86_64::` 36×; 1,807 `target_arch` lines (riscv64 667, x86_64 630); 123 `asm!` sites in 31 kernel files; `kmain` (`main.rs:151-1781`) has 124 cfgs and enforces "VT-d before any DMA driver" by position.
- `yield_cpu` (`task.rs:1378-1917`) has two near-duplicate switch bodies and three HAL entry points.
- x86 has two page-table paths (`paging.rs` raw PTE flags vs `hal::PageTableTrait`), and walkers copied into `task/user_copy/`.
- Three board-selection models with four accessor names.

## Requirements

- Functional: `kernel/src/arch/{x86_64,aarch64,riscv64}/` export the same function set (`irq`, `clock`, `power`, `pci_irq`, `iommu`, `smp`, `user_access`, `page_probe`, `boot::platform_init`) selected by one `cfg` at `mod arch`; `arch::boot::platform_init()` returns a typed `PlatformReady`; `kmain` keeps the arch-neutral tail; `hal::arch::SwitchPlan` with one `switch(plan)` per architecture and an RAII interrupt-state type; x86 `KERNEL_ROOT` uses the HAL page-table trait; `leaf(root, va)` and `current_root()` live in the HAL; one `board::active()`; unimplemented HAL traits deleted (or implemented where one line suffices); 32-bit targets per decision 4.
- Non-functional: move-first, then unify; no behaviour change; CI guard caps `target_arch` occurrences outside `arch/` and ratchets down.

## Architecture

```
kernel/src/arch/mod.rs:  #[cfg(target_arch="x86_64")] mod x86_64; pub use x86_64::*;   (one cfg per arch)
kmain: arch::boot::platform_init() ─► PlatformReady { console, irq, clock, pci, iommu } ─► neutral init
sched: plan = SwitchPlan { out, in, root, asid, irq } ; arch::switch(plan)
```

`PlatformReady.iommu` being a required field encodes the ordering that is positional today.

## Assumptions

- **Claim:** the two `yield_cpu` bodies differ only in root/ASID handling, so a null root expresses the SAS case.
  **Confidence:** low
  **How to verify:** diff the two bodies line by line before designing `SwitchPlan`.
- **Claim:** `scripts/check-hal-boundaries.sh` can host the `target_arch` ratchet.
  **Confidence:** medium
  **How to verify:** read the script.
- **Claim:** x86 raw-PTE path has no flag the HAL trait cannot express.
  **Confidence:** low
  **How to verify:** compare `page_protect.rs:229` `pte_bits_from_flags` with `hal/arch/x86/src/x86_64/paging.rs:385`.

## Related Code Files

- Create: `kernel/src/arch/{mod.rs,x86_64/,aarch64/,riscv64/,legacy/}`
- Modify: `kernel/src/main.rs`, `kernel/src/board.rs`, `kernel/src/platform.rs`, scheduler switch path, `kernel/src/memory/{paging,page_protect}.rs`, `kernel/src/task/user_copy/**`, `hal/core/src/lib.rs`, `hal/arch/*/` (add `SwitchPlan`, page probe, user-access window), `hal/core/Cargo.toml` and `kernel/build.rs` (default arch → x86_64), `scripts/check-hal-boundaries.sh`, `docs/specs/04-hardware.md`, `docs/system-architecture.md` (board-feature sentence)
- Delete: `hal/traits/{timer,interrupt}` and `BootController` if left unimplemented, `ViHypervisor` stubs, `boards` `aarch64_catalog()` if unused

## Implementation Steps

1. Record the baseline: `target_arch` count per directory; add the ratchet guard at today's numbers.
2. Create `arch/` and move code file-by-file (asm sites, direct `hal::<arch>` uses) with no logic edits; lower the ratchet each commit.
3. Split `kmain` into per-arch `platform_init` + neutral tail; x86 first, boot lane after each arch.
4. `SwitchPlan`: diff the bodies, implement per arch, keep the old path behind a test-posture A/B switch for one cycle, then delete.
5. Page tables: route x86 shared root through the HAL trait; move walkers and current-root readers into the HAL; rename Sv39-specific vocabulary.
6. Unify board accessor; delete dead traits; flip defaults to x86_64; quarantine or keep 32-bit code per decision 4.
7. Correct Spec 04 §1/§7 to the real seam.

## Todo List

- [x] Ratchet guard
- [ ] Code moved into `arch/`
- [ ] `kmain` split
- [ ] `SwitchPlan`
- [ ] Single page-table path
- [ ] Board accessor, dead traits, defaults

## Success Criteria

- [ ] `target_arch` occurrences outside `kernel/src/arch/` reduced by at least half from the recorded baseline, and the guard fails when one is added.
- [ ] No `asm!` outside `kernel/src/arch/` and `hal/`.
- [ ] x86 1/2/4-CPU domain lanes, RV64 `--harts 2`, AArch64 `virt-vhe` 2-CPU lane pass; context-switch A/B fixture shows identical markers before the old path is deleted.
- [ ] Kernel builds for all three 64-bit targets with no `cfg_attr(not(target_arch…), allow(dead_code))` added.

## Risk Assessment

- Context switch and paging are the highest-risk code in the kernel; the A/B cycle and per-arch boot lanes are mandatory, not optional. Rollback: revert per step; nothing persistent.
- AArch64/RV64 have less CI coverage on hosted runners; run their SMP lanes locally and record logs.

## Security Considerations

A single page-table path removes the chance that Tier-1 and Tier-2 mappings disagree on flag encoding. No authority change.

## Next Steps

Phase 15 ports Tier-2 grants to x86 using the neutral copy/probe interface.

## Deviation Log

- **Decision (step 1, 2026-11-13):** census in [research/p14-census.md](research/p14-census.md). Ratchet added to `scripts/check-hal-boundaries.sh` at 1 837 `target_arch` lines / 124 `asm!` sites outside `kernel/src/arch/`.
- **Surprise (step 1):** 32-bit targets do not build today (riscv32 17 errors, arm 13, x86-32 stops in `libs/abi` on the `ViVmExit` size assert). "Build-check only" for `arch/legacy` is therefore not available without an ABI change; the code is quarantined unbuilt, as before. Not fixed here.
- **Decision (step 1):** the 486 lines of `any(riscv64, aarch64, x86_64)` exist only to exclude 32-bit; `build.rs` emits `cellos_wide` (same set as `posture.rs`) and they collapse to it. Counted toward the ratchet.
- **Decision (step 1):** `asm!` sites outside the nine function sets get `arch::{tlb, cpu, pio}`; hypervisor-owned asm moves with its module.
- **Surprise (step 1):** the kernel already has `sched::domain_switch::SwitchPlan`; the HAL type needs another name or the old one is renamed. Body B of `yield_cpu` lacks the dispatch marker, and only builds for 32-bit/compile-only; x86 does not write CR3 in the switch (root reaches user entry through `gs:[24]`).
- **Decision (step 1):** `kmain` splits in two arch phases (`early()` → heap → `platform_init()`); `PlatformReady.iommu` is optional (aarch64 starts VirtIO DMA with no IOMMU).
- **Note (step 1):** RV64 `--harts 2` and AArch64 test-hooks lanes were red at Phase 10 base; baselines are taken before step 4.
