---
phase: 1
title: Tier-1 SAS caller-owned writable-range authority
status: user_authorized_pending_named_records
priority: P1
---

# Phase 01: Authority Contract

## Context Links
- `docs/specs/05-application.md`
- `docs/specs/12-reliability.md`
- `kernel/src/task/tcb.rs`
- `kernel/src/task/stack.rs`
- `kernel/src/loader/elf.rs`

## Overview
Define the ownership authority needed to distinguish a caller's writable memory from globally mapped peer memory in Tier-1 SAS.

## Key Insights
SAS page mappings establish neither Cell ownership nor delegation. The existing per-cell heap is static writable ELF storage, while user stacks and grant ranges have distinct lifecycles.

## Requirements
- Authorize an output span only when every byte belongs to the caller's live Cell or a currently owned mutable grant.
- Preserve ownership across same-Cell threads and reject every peer Cell range.
- Keep range records synchronized with load, stack creation, grant transfer/revocation, and retirement.
- Return a Cell-generation write lease that remains held from the final authorization through byte commit.
- Mark retiring/revoked records unavailable, then drain their write leases before unmap, reassignment, or reuse.
- Permit contiguous adjacent records only when their owner, generation, writability, and live state match.
- Do not treat page-table USER/WRITE bits as ownership.

## Architecture
Root Cell final-writable ELF pages + per-task usable user stack + caller-owned mutable grants form the source-of-truth record sets. A no-allocation cursor walks their contiguous union only while owner/generation/liveness match. Final authorization acquires `PAGE_GRANT_TABLE → REG_GRANT_TABLE → SCHEDULER`; a retiring caller/root is rejected, and the ordered locks remain held through the checked commit. Grant removal and scheduler retirement must acquire their member lock before unmap or reuse.

## Related Code Files
`kernel/src/task/tcb.rs`, `kernel/src/task/stack.rs`, `kernel/src/task/elf_prepare.rs`, `kernel/src/task/syscall.rs`, `kernel/src/task/drivers/virtio_rng.rs`, grant lifecycle sources.

## Implementation Steps
1. Preserve final-writable ELF-page metadata in `CellSegments`.
2. Use each task's usable-stack bounds and caller-owned grants as record sources.
3. Walk contiguous source records without heap allocation, rejecting gaps and peers.
4. Retain the ordered lock scope through the final checked write.
5. Keep named PAL/runtime/security admission separate from the user-authorized bounded child.

## Todo List
- [x] Record writable final ELF pages, stack bounds, and caller-owned grant sources.
- [x] Define no-allocation union coverage and the `PAGE_GRANT_TABLE → REG_GRANT_TABLE → SCHEDULER` final-lease order.
- [ ] Obtain named PAL/runtime/security approvals and rebind the PAL manifest.
- [ ] Demonstrate removal/revocation races through the isolated QEMU evidence matrix.

## Success Criteria
- A peer, kernel, stale-generation, revoked-grant, guard-page, or read-only range cannot authorize output.
- Final authorization rejects retiring callers and roots, and holds every contributing source lock through the copy.
- No PAL, runtime, promotion, or named-owner approval is inferred from the user-authorized child.

## Risk Assessment
A ledger that tracks heap allocations through the kernel global allocator would misattribute kernel allocations during syscalls. Track Cell image/stack/grant sources instead. A lookup-only design creates a TOCTOU write into retired or reassigned memory; leases must bridge final authorization and commit.

## Security Considerations
Record lookup and leasing must fail closed on missing, overlapping, stale, partially covered, dying, or revoked ranges. No raw pointer access occurs during authorization.

## Next Steps
Obtain approvals, then implement the record authority before modifying `GetRandom`.
