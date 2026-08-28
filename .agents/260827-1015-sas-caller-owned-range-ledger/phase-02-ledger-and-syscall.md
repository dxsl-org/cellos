---
phase: 2
title: Enforce owned GetRandom output
status: runtime_evidence_complete_approval_pending
priority: P1
dependencies: [1]
---

# Phase 02: Ledger and Syscall Enforcement

## Context Links
- `phase-01-authority-contract.md`
- `kernel/src/task/syscall.rs`
- `libs/ostd/src/syscall.rs:1735-1758`

## Overview
Wire the approved authority into `GetRandom` without changing its ABI or adding architecture-specific rejection behavior.

## Key Insights
`GetRandom` previously capped before descriptor validation and reached the copy only when entropy existed. The bounded implementation validates the original descriptor, then preflights the capped span before entropy, including the production zero-byte path.

## Requirements
- Validate original `(buf_ptr, len)` against `MAX_USER_BUF` before capping.
- Preserve `min(len,64)` write semantics for valid larger buffers.
- Authorize the complete capped output span before invoking the RNG.
- After entropy acquisition, obtain a second authorization that returns a Cell-generation write lease and retain it through the ≤64-byte checked commit.
- Removal, unmap, reassignment, and reuse paths must mark the record dying/revoked and drain write leases first.

## Architecture
Original descriptor arithmetic validation → capped output span → ownership preflight under record locks → RNG → repeated contiguous-union authorization under `PAGE_GRANT_TABLE → REG_GRANT_TABLE → SCHEDULER` → checked copy while all contributing records remain locked → return count or ABI error.

## Related Code Files
`kernel/src/task/syscall.rs`, `kernel/src/task/stack.rs`, `kernel/src/task/elf_prepare.rs`, `kernel/src/task/copy_glue/mod.rs`, `kernel/src/task/user_copy/copy.rs`, `libs/ostd/src/syscall.rs`.

## Implementation Steps
1. Preserve each final-writable loaded ELF page in `CellSegments`.
2. Validate the full original descriptor, then calculate `min(len,64)`.
3. Preflight contiguous caller-owned coverage before entropy lookup even when it returns zero.
4. Reacquire all contributing record locks in established order, reject retiring roots/callers, and copy under that scope.
5. Leave Domain and non-RV paths on their existing copy semantics.

## Todo List
- [x] Implement the source-record authority and final lease scope.
- [x] Change `GetRandom` ordering without changing valid-large-buffer behavior.
- [x] Add no-allocation union traversal and explicit lock-order documentation.
- [x] Complete hostile/revocation runtime matrix and bind approval evidence.

## Success Criteria
- A zero-byte RNG result never turns an invalid or peer pointer into `Ok(0)`.
- Valid 65–`MAX_USER_BUF` buffers receive no more than 64 bytes.
- Adjacent live same-owner records may span the output; foreign, retiring, or gapped records cannot.
- Focused QEMU runtime proof covers final-lease races against real root retirement, grant revocation, exact-frame unmap/reuse, and replacement-byte clearing; PAL approval remains external.

## Risk Assessment
A second lookup without a retained lease creates a TOCTOU window against retirement, grant revoke, unmap, and reuse. Lease acquisition, removal transitions, and the checked copy must not deadlock against scheduler, grant, loader, or retirement locks.

## Security Considerations
The function fails closed for absent, stale, partial, dying, or revoked records. It must not substitute mapping permission for ownership; only a final authorization lease permits the raw output commit.

## Next Steps
Retain the focused QEMU PASS record with the approval package; named PAL sign-off remains the only closure prerequisite.
