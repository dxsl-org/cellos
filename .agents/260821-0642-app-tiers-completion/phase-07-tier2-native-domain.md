# Phase 07 — Tier 2 Native Domain C6

## Context Links
`.agents/TODO.md:36-46`; `docs/specs/22-native-domain-cell-implementation-gate.md:26-31`; `docs/specs/22-native-domain-cell-implementation-gate.md:33-222`.

## Overview
Independently approve the high-risk private native-domain implementation child.

## Status, atomic prerequisite, and VFS closure boundary

**`ATOMIC_PUBLICATION_PREREQUISITE_COMPLETE / PHASE07_BLOCKED`** is verified for the narrowly scoped loader/task publication prerequisite. A fresh test-hooks build/sign passed; the populated-fixture one-hart VFS run passed `1/1` with `AP-00` through `AP-11` and `AP-15` passing and `AP-13` explicitly `SKIP`; and the distinct `-smp 2` atomic run passed `1/1` with `AP-00` through `AP-15`, `AP-02` PTE/TLB cleanup proof, the `AP-13` remote-hart witness, and both terminal markers.

`CELLOS-VFS-SMP-006` is separately closed at **`CELLOS-VFS-SMP-006_CLOSED_VERIFIED_RV64`**. It recorded API `90/0`, RV32 release compilation, fresh test-hooks, one-hart VFS `2/2`, and RV64 two-hart VFS lifecycle `7/7`, covering owner context, heartbeat retirement, quota fault, root exit, retiring syscall, leases, owner watches, AP/init, and VFS behavior. Its final quality and security closure are PASS; the source record and repository-reference hash `85a5b873c5961c911ea8e04473c4fcb61de68b4a` are in the [owner-lifetime closure record](../260822-cellos-vfs-smp-006-owner-lifetime/plan.md#closure-record--2026-08-22). RV32 runtime was unavailable only because host OpenSBI firmware is missing; that non-blocking evidence gap is not a claim of RV32 runtime success.

This closes `CELLOS-LOADER-RACE-002` and `CELLOS-LOADER-CLEANUP-003` only at the atomic-publication prerequisite boundary, and closes the separate VFS owner-lifetime ticket only. Full Phase 07 remains **blocked** on Phase 03, Phase 04, and independent Tier 2 qualification. It is not Tier 2 completion, readiness, release, ledger closure, or any approval.

## Key Insights
Tier 2 is an address-space mechanism; lifetime, quiescence, user copy, grants, and DMA form one boundary.

## Requirements
Kernel-owned address-space generation/lifetime; per-domain page/table/task/grant/pin quotas; SAS fast path; copied IPC; synchronous grant revoke; recoverable domain user copy; MMIO/IOMMU confinement; complete Spec 22 matrix across supported arches. A stuck hart or device fence timeout quarantines the domain, roots, frames, tags, grants, and DMA pins: never free/reuse before matching CPU and device acknowledgements. Require allocation/shootdown/revoke/fence/kill failure injection, canary cohort limits, bounded drain deadline, and separately approved rollback before exposure. Loader/task publication must be atomic: install and validate allowlist, quota, policy, capabilities, protection state, and singleton constraints before any task becomes ready/runnable; every denial must roll back task, quota, mappings, and scheduler state.

## Architecture
Policy/quota → domain builder → TCB publication → tier scheduler → IPC/grants → DYING/quiesce/fences → safe teardown or quarantine → canary/drain controller → ledger.

## Assumptions
`AddressSpace`, `native-domains`, `native-domain-admission` are provisional `[UNVERIFIED]`.

## Related Code Files
`kernel/src/memory/paging.rs:38`; `kernel/src/task/scheduler.rs:683-692`; `kernel/src/task/tcb.rs:141-149`; `kernel/src/task/syscall.rs:98-165`; `kernel/src/loader.rs:115-192`; `hal/arch/riscv/src/rv64/trap.rs:124-151`. The atomic-publication child verified closure of `CELLOS-LOADER-RACE-002` and `CELLOS-LOADER-CLEANUP-003` at its narrow boundary; Phase 03 retains `CELLOS-LOADER-SIG-001`, and this phase retains all independent Tier 2 qualification work.

## Implementation Steps
Design ownership/arches/quotas; enumerate pointers/callers; preserve the verified atomic loader/task publication boundary while designing Tier 2 admission; design teardown/quarantine state machines; define canary size and rollback bound; red-team/approve; build default-off; inject stuck-hart/device/allocation/final-admission failures; pass hostile matrix.

## Todo List
- [ ] Independent plan approved.
- [x] Atomic-publication prerequisite verified: `CELLOS-LOADER-RACE-002` and `CELLOS-LOADER-CLEANUP-003` close only at this prerequisite boundary.
- [ ] Full Tier 2 verification passes.
- [ ] Hostile evidence passes.
- [ ] UI hidden until qualification.

## Success Criteria
No SAS/peer/kernel/device access; no root/frame/tag/pin reuse before every fence; timeout quarantines rather than frees; quotas deny cleanly; no task is schedulable before all admission state is installed; every denial restores the pre-spawn scheduler/task/quota/mapping state; canary drain meets approved bound; SAS counter unchanged; no SAS fallback.

## Risk Assessment
Stale translation/root UAF or unresponsive hardware remains a full Tier 2 risk. `CELLOS-LOADER-RACE-002` (High) and `CELLOS-LOADER-CLEANUP-003` (Medium) are verified closed only for atomic loader/task publication; `CELLOS-VFS-SMP-006` is separately closed with the owner-lifetime evidence recorded above. None of those closures completes Phase 03 provenance/signature work, Phase 04 qualification, or any Tier 2 gate. Rollback stops admission, drains bounded canaries, quarantines non-acknowledged resources, and reboots policy-off; never frees-before-fence or converts to SAS. Quarantine may leak capacity until reboot/recovery, which is safer than reuse.

## Security Considerations
Missing negative evidence blocks release; CPU isolation does not imply DMA/side-channel isolation. Phase 03 must separately resolve `CELLOS-LOADER-SIG-001`; this phase cannot claim production loader readiness while that Critical provenance/signature-boundary risk remains open.

## Next Steps
Keep full Phase 07 blocked while Phase 03 and Phase 04 complete their required gates, then perform independent Tier 2 qualification. The VFS owner-lifetime ticket is closed; it does not satisfy any remaining Phase 07 dependency. Only completed direct dependencies and full qualification can permit Phase 08 real-design work.

## Deviation Log
None.
