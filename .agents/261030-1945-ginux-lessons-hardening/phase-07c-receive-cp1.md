# Phase 07C CP1 — post-start capability receipt (Law-1 semantic + new ABI)

Status: **draft, awaiting user approval.** No code. Selecting it is not approval.

## 1. Facts (verified 2026-10-31)

- After 07A, CSpace derive admits only the source incarnation itself (`AuthorityTable::derive_destination`, `kernel/src/cell/cspace.rs`). Cross-Cell grants are `Denied`.
- No syscall reaches `dispatch_handle_control` or `dispatch_cspace_control`; no userspace consumer of CSpace delegation exists.
- The only kernel resource held in CSpace today is the BootFS file (`OpenCap`), and it is non-delegable.
- Service resources (VFS directories, NET sockets) are issuer-local (Spec 25 §3.3); VFS directory receipt is spawn-only and service-enforced.
- Spec 25 §3.1: "After-start receipt requires explicit opt-in within that same fixed ceiling"; §3.2: delegation needs a delegable right, a named destination incarnation, subset rights and bounded lineage.

## 2. Question for the user

There is **no consumer** that needs cross-Cell kernel-capability transfer today. Building a receive path now adds ABI with no user. Options:

- **A. Defer (recommended).** Keep 07A fail-closed. Record that the receive path is designed when the first real consumer appears (for example, sharing a BootFS file read handle, or a driver handing a device grant to a client). Phase 07's "post-start receipt is forbidden unless opted in" is then satisfied by refusal.
- **B. Design now, implement later.** Accept the shape in §3 as the target contract without code, so a future consumer uses it.
- **C. Implement §3 now.** New syscall wiring, a receiver-side opt-in and host and QEMU tests, without a production consumer.

## 3. Proposed shape (for B or C)

R1. **Issuer-local record, holder-side exercise.** The grant stays in the delegator's table (07A D1). The receiver gets an opaque `(issuer cell, issuer generation, ticket)` reference. Invocation looks up the issuer's table and checks `holder == caller incarnation`, plus lineage, expiry and admission-cut, atomically under `CELL_AUTHORITIES`. No copy goes into the receiver's table, so revocation stays single-table and bounded.

R2. **Receive opt-in.** Each incarnation carries an immutable, spawn-time `receive_policy: kinds bitmask` (default empty), set from its launch profile or manifest and never widened after start. Derive to another Cell requires `kind ∈ destination.receive_policy` and `rights ⊆ destination ceiling for kind`; otherwise `Denied`.

R3. **Delivery.** The kernel attests the transfer to the destination through IPC, carrying sender identity and the reference. The receiver may close its reference. Close never revokes the delegator's lineage (Spec 25 §3.2).

R4. **Death.** If the issuer incarnation dies, every reference is `Stale` (issuer-local). If the receiver dies, its holder slots are cut at the issuer by a bounded per-issuer sweep at retirement, charged to the issuer's slot budget.

R5. **ABI.** New syscall (or HandleControl action 4 "Exercise"), plus a launch-profile field. Needs a Law-1 CP1/CP2 with exact layouts, and the lock register is unchanged (one lock).

## 4. Evidence plan (C only)

Host tests for opt-in absent/present, ceiling exceeded, stale issuer, stale receiver, close versus revoke, and lineage cut. A QEMU two-hart run of the witness with a receiver on hart 1.

## Decision (2026-10-31)

User chose **A. Defer**: derive stays fail-closed (07A); the receive path is designed when a real consumer appears. Phase 07 closed.
