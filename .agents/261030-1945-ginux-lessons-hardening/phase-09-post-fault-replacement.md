---
phase: 9
title: "Post-fault replacement and stash fencing"
status: pending
priority: P1
effort: "1.5w"
dependencies: [7, 8]
tier: thinking
---

# Phase 09: Post-fault replacement and stash fencing

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs.

## Overview

Ginux proves that a post-fault replacement is a fresh generation and that it recovers the exact committed record (`/tmp/ginux/ARCHITECTURE.md:154-155`, `specs/kernel/service-supervision-v0.md:105-114`). Cellos has two gaps:

1. **Stash isolation (security).** Syscalls `StateStash`/`StateRestore`/`StateStashClear` (`kernel/src/task/syscall.rs:7465-7552`) call the unversioned `stash`/`restore`/`remove`. A caller that guesses another Cell's key can read, overwrite or clear it. The versioned helpers (`state_stash.rs:169-212`) are used only by a test mirror (`tests/unit_cspace_smoke.rs:684-745`). Real consumers go through the public syscalls (`libs/ostd/src/hotswap.rs:70-113`).
2. **No faulted-provider replacement test.** `native_stateful` kills VFS rather than the provider, and runs on RV64 (`cells/tests/bench/src/scenarios/native_stateful.rs:128-260`).

This phase supplies P06 AC-14..AC-18 evidence on x86.

## Requirements

- Stash fencing at the syscall layer, without a public ABI change:
  - The kernel derives `(CellId, generation)` from `caller_id`.
  - Every non-argv StateStash, StateRestore and StateStashClear goes through one versioned owner-checked entry.
  - Read, write and clear succeed only for the stored owner `CellId`, with caller generation ≥ the latest writer generation. Older-generation writes and clears are refused (single fenced writer).
  - The cellbox `SPAWN_ARGV_KEY` special case is preserved as-is.
  - Stop if the syscall signature or error encoding must change (Law-1).
- The unused versioned helper API is folded into that entry: one stash implementation, no parallel path.
- Fault-replacement scenario on the **test-hooks lifecycle image from Phase 07** (1 and 2 CPUs), with a stateful provider (hotswap-demo counter or a dedicated test Cell). Fault injection via test hooks:
  - (F1) before commit;
  - (F2) after commit, before reply;
  - (F3) the Phase 08 AHCI driver faults with a notified command outstanding, after an earlier committed write.
- Expected outcomes:
  - F1: client sees `INDETERMINATE`; the replacement restores N−1; a retry with the same request id applies once.
  - F2: client sees `INDETERMINATE`; the replacement restores N; reconciliation returns the committed result without re-applying.
  - F3: the Phase 08 ordered markers appear, then the no-FLR outcome (the BDF is refused and the provider reports the device unavailable). If the NVMe FLR branch was proven in Phase 08, the replacement also reads the committed LBA there.
- Freshness assertions, by execution class:
  - **All:** new generation; CSpace containing only bootstrap grants; fresh task stacks and image mappings; empty operation state; new DMA lease id (device providers); old-generation handles, epochs and service-registry bindings refused.
  - **Domain Cells only:** a different address-space root.
  - **Tier-1 SAS Cells:** the shared root is acknowledged as by design (`loader/domain_admission.rs:179-201`), not asserted.
- Resource oracle: an explicit before/settled-after vector covering CSpace slots, live operations, stash entries, task stacks, pinned frames, quarantine entries, BDF quarantine states, and IOMMU domains/page tables. Any `unknown` value fails the check; it never counts as zero. P01's aggregate counters are not used; their qualification is open (controlling plan :287-293).

## Assumptions

- **Claim:** committed provider state for F1/F2 can live in the (now fenced) stash: it survives Cell death but not reboot. Reboot durability belongs to 261025. **Confidence:** high.
- **Claim:** request-id reconciliation exists in the P04 operation layer. **Confidence:** low. **How to verify:** read the P04 operation ledger and `libs/ostd/src/ipc.rs:41-47,124-126`. If absent, the test provider keeps a provider-local idempotency table, and the evidence ceiling says "provider-local idempotency only"; the P04 gap is recorded.
- **Claim:** each item in the resource vector has an exact source-owned counter or can be enumerated in test hooks. **Confidence:** medium. **How to verify:** list the source of each item before writing the scenario; any item without a source becomes `unknown` and fails.

## Related Files

- Modify: `kernel/src/task/syscall.rs` (stash arms), `kernel/src/cell/{state_stash.rs,hotswap.rs,service_registry.rs}`, `libs/ostd/src/hotswap.rs` (doc comments only, unless behavior changes), `tests/unit_cspace_smoke.rs` (drop the mirror, test the real entry), the test provider Cell, an x86 variant of `native_stateful`, and the test-hooks lifecycle runner.
- Docs: P06 phase evidence; Spec 25 recovery annex (stash ownership and fencing rule).

## Implementation Steps

1. Single owner-checked stash entry plus host tests: foreign read/overwrite/clear refused; stale-generation write/clear refused; same-generation and newer-generation restore OK; argv path unchanged.
2. Syscall arms routed to it; the old unversioned functions removed (clean cutover).
3. x86 stateful scenario with F1–F3 hooks; freshness assertions by class; resource vector.
4. Run on 1 and 2 CPUs (an AP-execution marker is required). Keep serial logs under this plan directory.

## Success Criteria

- [ ] Host stash tests pass (the 6 cases above), and a syscall-level negative test on x86 shows a foreign Cell refused for read, write and clear.
- [ ] F1, F2 and F3 pass with the stated outcomes on 1 and 2 CPUs.
- [ ] Class-appropriate freshness assertions pass. The old-handle negative control is refused, with a marker.
- [ ] Across 10 fault/replace cycles, the resource vector is equal before and after settling, with zero unknowns.

## Security Considerations

This fixes cross-Cell stash read/overwrite/clear. That is a live isolation bug independent of ginux, so it should go first if Phase 09 is split (log the split).

## Risk Notes

If reconciliation is provider-local only, F2's ceiling says so. Undo: revert. Stash contents are volatile, so there is no migration.

## Deviation Log

None.
