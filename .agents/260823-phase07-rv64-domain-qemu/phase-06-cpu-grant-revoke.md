---
phase: 6
title: "CPU-only DomainGrant revoke"
status: pending
priority: P1
effort: 3d
dependencies: [1, 2, 3, 4, 5]
tier: thinking
---

# Phase 06: CPU-only DomainGrant revoke

## Overview

Add the first explicit shared-memory exception only after copied IPC: one owner, one grantee,
page-aligned receiver PTEs, and synchronous CPU revoke. DMA remains denied and quarantined.

## Requirements

- Define `DomainGrant { owner: DomainRef, grantee: DomainRef, range, receiver_va, perms,
  generation, state: Live|Revoking|Revoked }`; creation verifies exact owner/grantee liveness,
  private ledger ownership, alignment, non-overlap, and one mapped grantee.
- The current SAS `GrantShare`/identity mapping is not extended or accepted across a domain.
  Receiver mappings use `AddressSpace::map_grant`; owner pages are never globally exposed.
- Revoke linearizes `Live→Revoking`, blocks new maps and IPC use, removes receiver PTEs,
  invokes local/remote ASID invalidation, waits for every reported current hart to acknowledge
  the matching domain/generation from the safe root, then marks Revoked. Only then may frame
  ownership be released.
- Explicit unshare, owner/grantee exit, forced exit, fault, failed spawn, policy draining, and
  address-space destruction enter the same deferred state machine. Active fault paths queue
  teardown after a safe-root return; they never free their active root inline.
- This is CPU-only. Any DMA-pinned grant is quarantined after CPU unmap and cannot recycle
  until existing IOMMU/device fence acknowledgement; initial domain admission denies DMA,
  so no DMA-positive claim or feature is introduced.

## Architecture

`Live → Revoking → remove PTEs/fence → matching safe-root acknowledgements → Revoked → frame/root release`; terminal and explicit paths share the same deferred queue.

## Assumptions

- **Claim:** SBI remote fence can provide an RV64 QEMU two-hart acknowledgement transport.
  **Confidence:** medium
  **How to verify:** Phase 07’s required non-SKIP two-hart revoke run.

## Related Files

- Modify: `kernel/src/task/syscall.rs`, `kernel/src/task/tcb.rs`, `kernel/src/task/scheduler.rs`,
  `kernel/src/memory/address_space.rs`, `kernel/src/memory/tlb_shootdown.rs`.
- Create: `kernel/src/task/domain_grant.rs`, `kernel/src/task/domain_grant_tests.rs`.

## Implementation Steps

1. Keep legacy RegGrant/PageGrant isolated; add a distinct typed syscall-internal path only
   reachable from the Phase 05 fixture policy, with no public ABI expansion until approved.
2. Implement the generation lock and quiesce work queue; queue removal precedes safe-root IPI,
   acknowledgement, PTE removal/fence, leaf release, intermediate release, root release.
3. Use SBI remote fence with a target hart snapshot; reject stale or duplicate acknowledgement
   and retain frames on timeout/failure rather than guessing completion.
4. Integrate owner/grantee terminal paths and copy-reader drain without lock-order inversion.
5. Emit `S22-RV64-GRANT-REVOKE: PASS`, `S22-RV64-GRANT-RACE: PASS`, and
   `S22-RV64-DMA-QUARANTINE: DENY` in test evidence only.

## Test Matrix

| Runner | Cases | Gate |
|---|---|---|
| `cargo test -p cellos-kernel domain_grant --features native-domains,test-hooks` | state machine, map validation, owner/grantee death, no DMA enablement | non-QEMU |
| `bash scripts/qemu-native-domain-test.sh --harts 1 --case grant-revoke,forced-exit` | local PTE removal and root-last destruction | RV64 QEMU, 1 hart |
| `bash scripts/qemu-native-domain-test.sh --harts 2 --case grant-race,asid-reuse,forced-exit` | remote execution cannot access after revoke completion | RV64 QEMU, 2 harts |

## Success Criteria

- [ ] Revoke success proves no receiver translation or frame reuse before matching-hart acks.
- [ ] All destruction paths share one generation-checked deferred teardown protocol.
- [ ] DMA/MMIO/virtio-MMIO requests are denied; no claim extends beyond CPU mapping revoke.

## Security Considerations

A remote fence request is not an acknowledgement. Timeout leaks/quarantines rather than frees.
DMA completion is a different authority and cannot be implied by CPU revoke.

## Risk Notes

The existing IOMMU teardown is a precedent, not a substitute. QEMU two-hart scheduling is
necessary evidence for this phase but cannot close physical-DMA or larger-topology risk.

## Deviation Log

None.
