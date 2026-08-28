---
name: SAS caller-owned range ledger
status: runtime_evidence_complete_approval_pending
created: 2026-08-27
---

# SAS Caller-Owned Range Ledger

Close `PAL-031` without changing the frozen `GetRandom` ABI: callers may pass buffers larger than 64 bytes; the kernel writes `min(len, 64)` bytes.

| Phase | Status | Dependency | Outcome |
|---|---|---|---|
| [01](phase-01-authority-contract.md) | user-authorized child; named records pending | PAL and runtime approvals | Define ledger authority and lifecycle invariants |
| [02](phase-02-ledger-and-syscall.md) | runtime evidence complete; approval pending | 01 | Enforce caller-owned writable output before entropy lookup |
| [03](phase-03-hostile-evidence.md) | hostile matrix passed; approval pending | 02 | Prove direct-syscall hostile cases and preserve ABI |

## Decision

The selected direction is a caller-owned range ledger derived from the live task stack, root writable ELF-page, and owned-grant records. A page-table writability probe is insufficient in Tier-1 SAS because peer Cell pages are globally mapped. Final authorization repeats contiguous union coverage under the established `PAGE_GRANT_TABLE → REG_GRANT_TABLE → SCHEDULER` lock order, held through the bounded checked write so concurrent removal cannot unmap or reuse a contributing range.

## Boundaries

- No new target triple, PAL, runtime, or production-admission claim.
- Do not reject valid buffers merely because `len > 64`; validate the original descriptor against `MAX_USER_BUF`, then authorize only the capped output span.
- Do not make non-RV64 callers fail solely to compensate for missing RV64 checks.
- Build begins only after the named Phase-02 PAL approvals and rebinding of the stale approval manifest.
- Approval sequencing is explicit: a named-owner authorization must permit this bounded kernel ledger/evidence child before its backing exists, while reserving PAL/runtime/promotion approval until its evidence and final manifest are complete.

## Dependencies

`kernel/src/task/syscall.rs`, `kernel/src/task/tcb.rs`, `kernel/src/task/stack.rs`, `kernel/src/loader/elf.rs`, `kernel/src/memory/*`, `libs/ostd/src/syscall.rs`, and the PAL approval package.

## Acceptance

`GetRandom` rejects null, overflow, above-`MAX_USER_BUF`, unmapped, kernel, peer, read-only, and unowned ranges before entropy lookup, including when the RNG returns zero. A second authorization returns a Cell-generation write lease held through the ≤64-byte commit; retirement, revoke, unmap, and reuse first mark their record dying/revoked and drain such leases. Valid same-cell stack, writable segment/static heap, owned grant, and adjacent same-owner cross-record ranges retain the `min(len,64)` ABI.
