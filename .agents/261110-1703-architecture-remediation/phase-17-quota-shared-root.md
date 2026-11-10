---
phase: 17
title: "Quota ownership, Tier-3 charging, shared-root hardening"
status: pending
priority: P2
effort: "8–12d"
dependencies: [16]
tier: thinking
checkpoint: false
---

# Phase 17: Quota ownership, Tier-3 charging, shared-root hardening

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs.

## Context Links

[Review Medium memory](../reports/architecture-review-261110.md) · `kernel/src/memory/{heap,cell_quota,kaslr,paging}.rs` · `kernel/src/loader/va_alloc.rs` · `kernel/src/task/stack.rs` · `kernel/src/hypervisor/{registry,svm_registry}.rs` · `kernel/Cargo.toml` (`x86-smep/smap/umip`) · [Spec 02](../../docs/specs/02-memory.md) · [Spec 19](../../docs/specs/19-hardware-isolation-layers.md)

## Overview

Charge memory to an explicit owner instead of "whoever is current", bring guest RAM and domain page tables under quota, and remove the construction-level limits the identity-mapped shared root puts on ASLR, W^X and SMAP.

## Key Insights

- Reviewer-cited: the allocator charges the cell current on the hart (`heap.rs:171`); at least 14 sites pin identity to 0 to avoid mis-charging; ISRs allocate (`heap.rs:232`); `address_space.rs` contains no charge; no `quota`/`charge` reference in `hypervisor/`, `ept.rs`, `stage2.rs`; VMs are keyed by owner tid; three separate tag allocators.
- `kaslr_cell_base` has no caller; image bases come from a deterministic bump allocator (`va_alloc.rs:140-146`); stacks and grants sit at their physical address; free frames are kernel-RWX by invariant; SMEP/SMAP are off in every shipped profile.
- The controlling plan's own P01 notes reached the same conclusion: current byte-only release tokens cannot prove ownership.

## Requirements

- Functional: an owner-tagged allocation API (`alloc_for(owner)`) and per-object reservations; ambient charging removed; `FrameRun` carries its charge owner; guest RAM, EPT/Stage-2 tables and domain page tables charged to the owning Cell incarnation; VMs keyed by `(cell, generation)`; one tag allocator; a VA allocator for stacks and grants (no longer VA==PA); free frames kernel RW+NX; shared-root commit wrapped in `stac/clac` so SMAP can be on; slot order randomised from kernel entropy when available.
- Non-functional: quota refusals stay before effect; no hidden allocation in interrupt context; changes land behind per-step lanes because each affects every cell.

## Architecture

- Charge owner is explicit data on the allocation, not thread-local state: `Charge { cell, generation }` passed at reservation; freed against the same record regardless of who frees.
- VA allocator: reuse the slot scheme in `va_alloc.rs` with separate arenas for images, stacks and grants; removes the contiguous-physical-run requirement Spec 19 §3 names.
- SMAP: user-access window type from Phase 14 (`arch::user_access`) becomes the only place AC is set.

## Assumptions

- **Claim:** Tier-1 cells do not rely on stack/grant VA equalling PA (e.g. passing addresses to drivers for DMA).
  **Confidence:** low
  **How to verify:** grep driver cells for use of a grant's address as a DMA/physical address; any such use must go through the existing DMA mapping syscalls first.
- **Claim:** x86 SMEP/SMAP/UMIP can be enabled on the QEMU CPU models the lanes use.
  **Confidence:** medium
  **How to verify:** existing `qemu-idt-protection-test.sh` lane and the Ginux Phase 04 notes on the feature-gated SMAP window.
- **Claim:** the fixed `IN_USE: [_; 64]` quota array (`cell_quota.rs:23,65`) is not relied on by measurement lanes beyond the documented D5 feature.
  **Confidence:** medium
  **How to verify:** grep `MAX_CELLS`.

## Related Code Files

- Modify: `kernel/src/memory/{heap,cell_quota,frame_run,address_space,kaslr,paging,ept,stage2}.rs`, `kernel/src/loader/va_alloc.rs`, `kernel/src/task/stack.rs`, the 14 pin-to-0 sites (`cell_quota.rs:200-211`, `ipc_wire.rs`, `pending_mailbox.rs`, `async_ipc.rs`, `completion.rs`, `pipe.rs`, `scheduler.rs`), `kernel/src/hypervisor/{registry,svm_registry}.rs`, `kernel/src/task/user_copy/guard.rs`, `kernel/Cargo.toml`, `docs/specs/{02-memory,19-hardware-isolation-layers}.md`
- Create: `reports/phase-17-evidence.md`

## Implementation Steps

1. Owner-tagged allocation API; convert the 14 pin sites to explicit owners; delete ambient charging; fix Spec 02 §2.
2. Charge domain page tables and hypervisor memory; key VMs by incarnation; single tag allocator.
3. Free-frame normalisation to RW+NX; boot all three arches.
4. VA allocator for stacks, then grants; audit driver cells for VA==PA assumptions first (assumption 1).
5. Enable SMEP, then SMAP with the access window, then UMIP, one lane cycle each.
6. Randomise slot order; wire `kaslr_cell_base` or delete it.

## Todo List

- [ ] Explicit charge owner
- [ ] Hypervisor and domain tables charged
- [ ] Free frames RW+NX
- [ ] VA allocator (stacks, grants)
- [ ] SMEP/SMAP/UMIP on
- [ ] Slot randomisation

## Success Criteria

- [ ] Fixture: an allocation made in interrupt context while Cell A runs is not charged to Cell A.
- [ ] Fixture: a VM whose guest RAM exceeds its owner's quota is refused before any mapping.
- [ ] x86 lane boots with SMEP+SMAP+UMIP enabled; a deliberate kernel dereference of a user address outside the access window faults (existing IDT-protection lane extended).
- [ ] Two boots of the same image place a named cell at different bases when entropy is available.
- [ ] Kernel host tests; x86/RV64/AArch64 boot and domain lanes pass.

## Risk Assessment

- Dropping VA==PA can break a driver that leaked the assumption: audit first, land stacks before grants. Rollback: each step is separately revertable; the feature flags for SMEP/SMAP remain as switches for one cycle.
- Quota behaviour changes for real workloads (kernel bookkeeping no longer billed to the wrong cell): limits may need re-measurement; record, do not tune silently.

## Security Considerations

Converts several audit Low/Medium items that share one cause (02 S-7, S-8, S-12; 05 S-14) into a single structural fix; closes unbudgeted guest RAM.

## Next Steps

Phase 18.

## Deviation Log

None.
