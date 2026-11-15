---
phase: 17
title: "Quota ownership, Tier-3 charging, shared-root hardening"
status: in-progress
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

- **Decision (step 1, 2026-11-15):** the census is in [research/p17-census-charge.md](research/p17-census-charge.md) (17a) and [research/p17-census-shared-root.md](research/p17-census-shared-root.md) (17b).
  - There are 31 production cell-identity swap scopes in 11 files, not 14. Seven of them only free memory and already do nothing for accounting, because `dealloc` refunds the recorded owner.
  - The timer interrupt runs the scheduler's housekeeping billed to the interrupted cell.
  - Explicit refunds ignore the generation.
  - VMs are keyed by owner tid. x86 VMX uses no VPID, so there are three tag allocators.
  - Spec 02 §2 states an outdated charging rule.
  - Assumption 1 is false for grants (six no-IOMMU DMA paths program the grant id directly, and `libs/abi` calls it a physical address) and true for stacks.
  - Free frames become RW+NX through two flag edits on RV64/AArch64. x86 has no identity alias.
  - The x86 user-copy path never sets AC, and six sites touch user memory outside any window.
  - `kaslr_cell_base` cannot be wired up as it stands. Only x86 has a kernel entropy source.
- **Decision (owner, 2026-11-15, census):**
  - Only stacks move off VA = PA in this phase. Grants stay VA = PA, recorded as a residual: moving them needs ABI text or a translation syscall.
  - SMEP/SMAP/UMIP are enabled when the CPU has them and logged when it does not. Only a build marked "required" refuses to boot. The lanes add `+smep,+smap,+umip` to prove the bits are on.
  - The ELF spawn's unzeroed user stack (`task/elf_prepare.rs:246-256`, a cross-cell information leak) is fixed now, in its own commit.
  - Grant pages, root stacks and cell segments stay uncharged, recorded as a residual.
  - `create_vm` returns the existing `OutOfMemory` for a budget refusal; its other errors keep `NotSupported`.
  - The stale charging comment in `libs/abi/src/telemetry.rs` is corrected (comment only, no byte changes).
  - Guest-memory budgets: 1544 MiB for `/bin/hypervisor`, 1 MiB for `/bin/silo`, 0 for every other cell.
- **Decision (step 2, commit 1, 2026-11-15):** the seven drop-only pins are deleted (P4, P6, P8, P12, P20, P25 and the `release_pipe` half of P9). Each was read first: every one only drops memory, and `dealloc` refunds the owner word, so accounting does not change. No pin was kept.
  - `PendingMsgData::Heap` loses its `allocation_cell` field, which only fed its drop pin.
  - `cell_session::KernelBytes` is deleted. It existed only to drop under the kernel identity, so the staging bytes are a plain `Vec`.
- **Decision (step 2, commit 2, 2026-11-15):** `Charge`, `Account`, `Limits` and `Reservation` are in `memory/cell_quota.rs` (census §8.1). Quota rows carry the task's `cell_generation`, and `QUOTA_LIMITS` is a lock-free atomic array. The allocator still charges the dispatch identity in this commit.
  - `publish_prepared` and `SessionPublication::reserve` mint the generation (`tcb::next_cell_generation`) before the quota reservation and build the task with `Task::of_incarnation`.
  - `QuotaReservation::reserve` claims a slot with a CAS on its limit word. Pin P10 and the `QUOTA_LIMITS` lock rank are deleted. A limit is clamped to the 1 TiB byte field, so a `usize::MAX` registration reads back as 1 TiB (only the death-subscription self-test registers one).
  - Behaviour: a charge naming an incarnation that the slot does not hold is refused. A charge to an unregistered slot is not counted. Before, the heap counted it without a limit and DMA bounded it by the default quota. Launched cells always have a row; only kernel-spawned test tasks run unregistered.
  - Deviation from §8.1: `Reservation` also has `keep()`, and `refund(charge, account, bytes)` is crate-visible. A few charges outlive every object that could hold a reservation: SHM until retirement (`release_shm_for`), the creator's retained measurement entry (`PublicationClaim`), a committed DMA grant, and DMA export windows behind the `Platform` trait. `keep` leaves those bytes charged, and their explicit refund is generation-checked like every other. `Reservation::charge()` is not added because nothing reads it yet.
  - `cell_session` forfeit and `pipe::disown_cell` are deleted. A refund to a retired incarnation is a no-op, so nothing needs disowning. Held reservations sit in `_charge` fields, which is the codebase's drop-guard convention.
  - The DMA export `Platform` quota hooks take the holder `Incarnation` instead of a raw CellId. `DmaPublicationGuard` holds `Option<Reservation>`, and the retirement barrier passes `None`.
  - `Account::Guest` and `GUEST_IN_USE` are left for step 5, which wires their limit. No variant exists without its counter.
  - Fixtures that reserve a row and then spawn a task (RV64 `SMP-RETIREMENT`/`SMP-ROOT-EXIT-QUOTA` and `HEARTBEAT-TERMINAL-IDENTITY`) mint the generation first and give it to the task. Fixtures with a fixed parent generation register with that generation.
- **Decision (step 2, commit 3a, 2026-11-15):** the allocator charges the hart's `alloc_charge`, two words appended after `current_tls_base` (the asm offset assertions cover only the leading four fields). Only `heap::ChargeScope` writes it; a hart whose local state is not installed charges the kernel.
  - Behaviour: `charge_owner` now takes a `Charge` and goes through the same generation-checked `add` as `Reservation`. A heap allocation naming an incarnation the slot does not hold is refused, and an unregistered slot is not counted. The owner word format is unchanged.
  - Owner scopes: P1-P3 charge the sender the wire header names (CellId 0, a kernel sender, is the kernel). P5 and P7 charge `Charge::of(target)`, including the hot-swap migration target. P9 keeps the creator's `Reservation` and allocates the ring under a kernel scope. P15 charges the owning incarnation. Kernel scopes: P11, P13, P14, P19, P21-P24, P26-P31. P16 and P17 keep `set_current_cell_id(0)` as the trap-attribution handoff and open a kernel scope after it. Every pin was converted.
  - Behaviour (handoff §4): wire payloads, mailbox growth and per-cell completion queues move from the kernel to the sender, receiver or owner. A receiver at its limit now refuses a delivery that needs mailbox growth, including a kernel relay from the timer tick. Interrupt and scheduler housekeeping moves from the interrupted cell to the kernel.
  - Deviation: the G1 witness pushes in `scheduler.rs` (census note "pass 0 but stay inline") stay kernel-charged. Their payload is inline and a refused push is fatal (`WitnessFull`).
  - P14: `AsyncIndex::try_note_holder` is fallible and `async_ipc::note_holder(sched, tid, token)` handles a refusal for every caller by clearing the receiver's `current_async_op`. The operation then ends by deadline, cancel or owner death, like any unanswered one. `forget_task` uses `forget_holder`, which never allocates.
  - Interrupt entries: the eight in §8.2. A grep of the `#[no_mangle] extern "Rust"` entries the HAL calls found no other interrupt. The rest are synchronous exceptions (`vi_handle_page_fault`, `vi_terminate_on_*`, `vi_user_copy_guard_fault`, `vi_tlb_shootdown_test_fault`, `vi_emulate_el0_mmio_aarch64`) or queries, and x86 MSI delivery stays inside the HAL. From user mode they run with no scope open (kernel); from kernel mode they inherit the open scope. None was added.
  - `yield_cpu` opens a kernel scope after its entry mask and drops it after `tls::apply_on_resume`, before restoring the interrupt state. A task entered for the first time starts under the kernel charge its predecessor's `yield_cpu` set.
  - §2.3 rewrites that would fail under the new rule land here, not in 3b: the heap host test, the RV64 `SMP-ROOT-EXIT-QUOTA` saturation and the `SMP-FAULT-RETIREMENT` worker probe (each opens `Charge::dispatched()` as its syscall would, and the hand-off hooks check `alloc_charge`), the `cell_session` test-hook staging scopes, and the two `ipc_pending_selftest` quota fixtures (mailbox growth is now the receiver's, so they saturate after the container exists and compare the payload delta).
- **Surprise (step 2, commit 3a, 2026-11-15):** the host test `cell::service_registry::tests::commit_requires_exact_paused_provider` failed once and passed on two reruns. It shares the service registry with other tests and takes no test lock. The failure is unrelated to this change.
- **Decision (step 2, commit 3b, 2026-11-15):** fixture (a) is in `memory/heap.rs`. The host test opens cell A's syscall scope (limit 0), allocates under `ChargeScope::interrupt()` and checks the owner word, `in_use(A) == 0` and that A's scope refuses again afterwards.
  - The boot fixture `IRQ-ALLOC-CHARGE` runs from `kmain` right after `irq.enable(clock)` on every `test-hooks` image, because it needs the timer running on the boot hart. Earlier boot fixtures run with the timer off. It reserves a fresh CellId with limit 0, sets the boot hart's identity and a `Charge::dispatched()` scope to it, checks that A's scope refuses a probe, and arms a 64-byte `try_box` in `vi_timer_tick` (taken on that hart only, first thing after the interrupt scope). It spins for at most 500 ticks, runs one `yield_cpu` housekeeping pass with A's scope still open, then checks the probe succeeded, its owner word is `UNCHARGED_OWNER`, `in_use(A) == 0`, and that A's scope refuses again. Nothing logs or allocates infallibly while A's scope is open. It logs `[selftest] IRQ-ALLOC-CHARGE: PASS` or `FAIL <reason>`.
  - The boot context stands in for cell A inside a syscall, as §8.6(a) describes. On AArch64 and RV64 the first tick usually switches to the ready tasks, so the verdict is logged when the boot context next runs (when the harts go idle).
  - The marker is required by `scripts/x86/qemu-domain-test.sh` (marker list), `scripts/qemu-aarch64-test-hooks.sh` (domain marker list) and `scripts/qemu-native-domain-test.sh` (every case boot, after the `S22-RV64-…: FAIL` sweep). The RV64 SMP retirement fixtures run in that image too.
  - The remaining §2.3 rewrites: `DEATH-SUBSCRIBER-QUOTA` subscribes and drains under the owner's and the exiting cell's scopes, and the AP-15 arming regression arms under a freshly reserved cell's scope (with the quota lock gone, it now proves arming does not depend on a kernel charge). The scheduler host tests at `scheduler.rs` touch only the dispatch identity and are unchanged. The RV64 retirement and heartbeat fixtures commit 2 touched read the identity only; their quota probes moved to scopes in 3a.
  - Spec 02 §2 states the explicit-charge rule and the actual OOM policy (null, `OutOfMemory` for fallible callers, idle hart for infallible ones). The `libs/abi/src/telemetry.rs` comment changes text only.
- **Decision (parent, P7, 2026-11-15):** mailbox `Vec` growth (`PendingMailbox::try_push`) allocates under a kernel scope, not the receiver's, and `try_push` no longer takes a charge. Charging the receiver made a kernel relay (timer tick, UART/input) to a receiver at its limit refuse delivery, which is a failure mode beyond handoff §4. The mailbox is depth-bounded. P5 (the payload copy over 32 bytes) stays on the receiver. This supersedes the P7 owner scope and the "receiver at its limit refuses" behaviour logged for commit 3a. The two `ipc_pending_selftest` quota fixtures go back to limit 0 for inline delivery and check that exactly the payload is charged and refunded. Spec 02 §2 lists mailbox growth as kernel bookkeeping.
