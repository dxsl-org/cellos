---
phase: 3
title: "Make domain grants permission-accurate and synchronously revocable"
status: pending
priority: P1
effort: "split into owner mapping, publish, revoke and death changes"
dependencies: [2]
tier: thinking
---

# Phase 03: Production grant lifecycle

## Requirements / architecture
- Preserve public `Grant*` syscall numbers and result conventions (`libs/api`, `libs/types` unchanged). Existing `PageGrant`/`RegGrant` tables store `(owner, shared_to)` but no mapped receiver root/generation (`kernel/src/task/syscall.rs:122-145,1997-2105`). Domain receiver mappings must carry exact owner and receiver cell generation, recipient root, per-page range, rights, pin/DMA state and a revoking state **in one kernel-owned record**. Treat SAS owner and domain owner explicitly; `task/domain_grant.rs:1-123` is a one-page RV64 private-private selftest, not a drop-in for SAS-owner/multi-page public ABI. Either adapt its state machine to the table or integrate it behind the existing table; remove dead duplicate fixture when cut over.
- In `GrantAlloc` **and** `GrantRegister`, allocate/zero backing without publishing USER access into `KERNEL_ROOT` when the owner is a domain (`syscall.rs:343-405,6315-6349,6615-6638`). Preserve SAS owner identity semantics separately; a domain-owned page must stay supervisor-only in the SAS root. Transactionally map the returned physical-base ID as an owner-domain VA with RW+NX; if virtual identity cannot be represented without collision, deny before publishing. Fail/undo every page and release only after shootdown if any mapping fails.
- In `GrantSlice`, resolve rights: `ReadOnly` → R+NX; `ReadWrite` → RW+NX. Genuine write-only is not enforceable as an ordinary page on every target: refuse it for domain pairs until ratified semantics; first inspect existing `libs/ostd/src/fs.rs:437-440` and `cells/services/hypervisor/src/virtio_blk.rs:455-458` callers that pass WriteOnly while VFS reads. Migrate those callers to accurate rights before lifting gate. Never widen rights or return an unmapped address. Any `(target identity,generation,root,rights)` tuple change—including **same recipient RW→RO**—must revoke old PTE and shoot down before publishing new tuple; an old receiver write must fault without calling `GrantSlice` again.
- Revoke/free/unregister must block new slices and user copies, drain readers + VFS/DMA pins, synchronously unmap receiver PTEs and shoot down target roots, then unmap owner domain PTEs, restore SAS kernel mapping and finally release frames. If busy, return a bounded existing error **without freeing frames**; retain a revoking/quarantined record for retry/exit cleanup. Owner death, recipient death, non-VFS grant, VFS lease and concurrent `GrantDma` must use the same lifecycle. Keep current lock order `PAGE/REG_GRANT_TABLE → SCHEDULER → pin REGISTRY` (`syscall.rs:1938-1984`); do not take `SCHEDULER` under a domain ledger/FRAME_ALLOCATOR lock or wait for remote completion while holding a lock needed for safe-root ack. Design reservation/generation protocol and document the linearization point before implementation.

## Related files
`kernel/src/task/syscall.rs`, `kernel/src/task/domain_grant.rs`, `kernel/src/task/tcb.rs`/task retirement paths if required, `kernel/src/memory/{address_space.rs,pin.rs,tlb_shootdown.rs}`, `kernel/src/task/grant_reclaim_selftest.rs`, `libs/ostd/src/fs.rs`, `cells/services/hypervisor/src/virtio_blk.rs`, `tests/integration/tests/tier2_fault_isolation.rs`, `scripts/{qemu-native-domain-test.sh,build-native-domain-test-ci.sh}`, `docs/specs/{02-memory.md,17-ipc-wire-contract.md,22-native-domain-cell-implementation-gate.md}`.

## Implementation steps
1. Ratify the private cutover contract in Specs 17/22: source/target TID vs cell-wide root, SAS/domain pairing, ID/VA collision, NX/permission policy, page count and state/failure semantics; do not change `libs/api`/`libs/types`. Enumerate all consumers of raw grant address and add failure-before regressions for owner mapping, SAS USER exposure, RO-write, WO rejection, nth-page failure, re-share, revoke/frame reuse, exit and DMA pin.
2. Split backing allocation from SAS USER publication; keep domain-owned frames supervisor-only in global root, zero before mapping, then atomically map all owner-domain pages or undo. Add generation-tagged ownership for source and destination in authoritative tables and retain `Arc<AddressSpace>` while mapped. For SAS-only pairs preserve current zero-copy and quota/accounting; deny unsupported domain↔SAS pairs rather than silently granting global access.
3. Resolve permission from `shared_to` under the table lock, then authorize/commit mapping and optional VFS lease transactionally; no lease may remain after map failure. Handle `GrantAlloc` domain owner before publication.
4. Implement revoke across PAGE/REG grants and **all teardown paths**, including `clear_grantee_refs`/`reclaim_owned_grants`/orphan reaper. Mark `Revoking` under table lock, release locks for synchronous target-root shootdown, reacquire and free only after ack; a pending ack keeps backing quarantined and supports idempotent retry. Validate that new shares/slices refuse while `Revoking`.
5. Build/pack a **producer and receiver Tier-2 ELF pair** with required launch profiles/manifests/capabilities into per-architecture throwaway images; implement non-skipping fresh QEMU runners for exact public owner+receiver syscall path in this phase (the existing `cells/tests/tier2-smoke` only exercises its own grant, and `scripts/qemu-native-domain-test.sh --case grant-revoke` is a private fixture). Exercise RV64 1/2-hart, AArch64/x86 one-CPU RO-write attribution, no SAS-root USER exposure, same-recipient downgrade, re-share, peer death and forced churn; record distinct terminal markers. Keep phase-01 deny gate until the real paired test passes for the exact pair/architecture; unsupported combinations remain denied.

## Success criteria
- [ ] An allocated domain grant pointer is writable by its owner; transaction failure leaves no visible ID/PTE/frame leak.
- [ ] Domain-owned backing never has USER access in SAS root; RO receiver write faults, RW works, WO refuses on unsupported pairs and migrated VFS caller still reads correctly; map failure never succeeds with a raw pointer.
- [ ] After `GrantFree`, `GrantUnregister`, re-share or either peer's exit, old receiver address faults and reused frame bytes stay private even across 2 harts.
- [ ] VFS/DMA pin and copy lease prevent premature reuse; SAS-only grant clients and normal copied IPC keep working.

## Assumptions / risk / rollback
- [UNVERIFIED] Physical identity is a usable receiver VA under all supported domain layouts; verify against `USER_LIMIT` and occupied mappings before deciding whether ABI-compatible alternate VA exists. No public ABI change without the two Law-1 owner checkpoints. Rollback: phase-01 deny gate plus cold reboot; leaked contents or prior DMA writes cannot be undone. Missing remote completion blocks release; retain quarantined frames rather than treating timeout as success.

## Deviation Log
None.
