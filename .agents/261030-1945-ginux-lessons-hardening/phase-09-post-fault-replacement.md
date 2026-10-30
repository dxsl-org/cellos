---
phase: 9
title: "Authorized state handoff and SAS post-fault replacement"
status: pending
priority: P1
dependencies: [1] # 09A only; 09B requires 09A; 09C requires 09B and 08
tier: thinking
---

# Phase 09: Authorized state handoff and post-fault replacement

## Overview

Apply item 3 to a Cell incarnation, not a whole address space per service. All threads of a retiring SAS Cell, its exported pointers and its device users must quiesce; other admitted Cells continue only within the established safe-Rust/audited-substrate failure boundary.

## Observed state and threat scope

`StateStash/Restore/Clear` non-argv syscall paths call the global-key `stash/restore/remove` (`kernel/src/task/syscall.rs:7490-7546`); these map caller-chosen keys to bytes without holder checks (`kernel/src/cell/state_stash.rs:24-26,103-147`). `SPAWN_ARGV_KEY` is a separate task-owned path (`syscall.rs:7479-7488,7503-7521,7542-7543`) and must stay separate. Versioned helper metadata is not enforcement on the real syscall path. This is an authority gap even for safe Rust, but not evidence that Tier 1 promised protection from arbitrary malicious native code.

## Requirements and architecture

### 09A — independent early handoff-authority slice (depends only on 01)

- Inventory all stash consumers and the real hotswap/recovery coordinator. Bind state to a typed state-domain identity with issuer and object generation, not a caller-controlled hash/name, TID or a raw pointer.
- Ordinary caller reads/writes/clears require its live holder authority. Recovery uses an explicitly approved transfer bound to source incarnation, destination incarnation, state domain and a unique transfer generation. A different CellId can be the legitimate replacement; matching CellId or larger numeric generation is insufficient.
- Keep transport authority separate from business authority. The stash carries an opaque state-authority-attested envelope; it neither certifies business commits nor issues business writer epochs. The provider-specific state authority owns committed revision/schema validation, final-watermark fencing and fresh writer epochs in 09B (Spec 25:116-120).
- Transfer lifecycle: Prepared → Sealed → Consumed/Aborted. Source may write only before sealing; only the named destination may restore sealed bytes, and successful complete delivery plus acceptance linearizes consumption. Partial/faulted delivery must not silently consume state. Coordinator receives scoped status/abort authority, not payload-read authority; replace its current one-byte restore probe (`cells/services/supervisor/src/hotswap.rs:37-48`). Replaying a completed transfer returns only its terminal status, never another payload grant or writer epoch.
- One owned byte-storage implementation covers read/write/clear and charges bounded entries/bytes, transfer storage and terminal records. No first-writer-wins global-name ownership and no old/new versioned tables with competing truths.
- Preserve task-owned argv staging/inheritance independently. Migrate every real SDK/supervisor/provider consumer and its documentation. If public semantics, handles, errors or transfer proof require a Law-1 amendment, obtain it before implementation; unchanged opcode/layout is not a no-ABI exemption.

### 09B — software provider crash/restore witness (depends on 09A)

- Worker-crash retention may use independent RAM state; reboot/node-loss retention requires the existing CellosFS/durable-state owner (Phase 01 item 7). Do not claim RAM retention as durable storage.
- At the provider-specific state authority, fence the old writer and capture the final committed watermark before cutover; validate schema/revision and authorize the destination's fresh writer epoch there, never in generic byte transport. Commit `(authenticated caller/operation, method/schema, request identity, canonical payload or digest, mutation result, state revision)` atomically with the mutation before acknowledging. Reconciliation compares that binding: same ID with different payload or authority is refused, not replayed. An idempotency table that dies with the worker is insufficient. No generic transparent retry added to C2C.
- Fault before commit → restore old revision; after commit/before reply → restore committed revision and reconcile the same request without reapplying; corrupt/missing/wrong-schema/revision state → RecoveryFailed, no Ready publication.
- Freshness means new holder incarnation, CSpace contents derived only from fresh grants, threads/stacks/TLS, operation ownership and publication binding. SAS root remains shared. Domain root/mapping validity must not alias old authority; allocator address reuse after proven quiescence is allowed, so raw CR3/stack-address inequality is not an identity test.
- Reacquire issuer-approved handles; never deserialize live handles or reuse raw pointers/driver queues. Partial restore failures unwind or quarantine before Ready. A detected fault outside admitted isolation (unsafe shared corruption/kernel failure) requires node recovery, not a successful Cell restart.
- Enumerate every supported cross-Cell borrow/ring/buffer and its incarnation-bound lifetime/grant/pin record. Revocation prevents new admission; backing allocations remain charged until every CPU/borrower/device user acknowledges quiescence. Rust lifetimes alone do not enumerate live exports after abrupt retirement. Never force-unlock or reuse a shared mutex whose guard died with the Cell. Untracked raw exports or abandoned shared-lock invariants fall outside the restartable profile; fail closed and require node recovery where safe containment cannot be established.

### 09C — device-backed extension (additionally depends on 08)

Use the proven controller-specific retirement/reset strategy to replace a driver with a fresh lease and read the committed record. Do not make 09A/09B wait for physical/FLR support. Device unavailability is a real result, not successful replacement evidence.

## Related Files

`kernel/src/cell/state_stash.rs`, real syscall arms, hotswap/publication coordinator and all actual stash SDK/test consumers found by LSP/reference census. Reuse an adequate isolated lifecycle runner (optionally Phase 07's fixture); missing fixture work is part of this slice, not a dependency on unrelated authority changes. No kernel-wide generic state service or duplicate issuer ACL.

## Implementation Steps

1. Census consumers, authority and transfer contracts; secure exact admission/Law-1 approvals for 09A.
2. Implement the authorized byte-transfer lifecycle and migrate all consumers, including supervisor status polling. Test foreign read/write/clear, stale/misbound transfer, guessed keys, repeated terminal queries and partial delivery; retain valid planned replacement and argv controls. Business fencing remains at the provider-specific authority.
3. Run 09B on actual 1/2-CPU workload with AP work: multiple threads, in-flight operation, tracked pinned/exported SAS state, before/after-commit faults, failed restore and delayed old writer. Exercise same-ID/different-payload and same-ID/different-authority rejection. Inject missing borrow acknowledgement and an abandoned shared-lock/export case to prove quarantine or node-failure classification, not unsafe reclaim.
4. Observe actual result/state through consumers, not only diagnostic markers. Record before/settled-after owned resource vectors; quarantine may legitimately retain charged resources after fault, must be explicit and bounded. Unknown accounting fails the no-leak claim, not zero.
5. Run 09C after 08. Run durable two-boot/power-cut recovery only through the admitted storage owner; absence leaves that separate profile unqualified.

## Success Criteria

- [ ] 09A refuses unauthorized reads/writes/clear and misbound transfer; only the destination receives payload, coordinator can query/abort without reading it, failed partial delivery preserves recoverability, and consumed replay cannot deliver twice. Valid cross-Cell replacement and argv inheritance work.
- [ ] 09B restores exact committed business state/outcomes at the actual state authority; old writer cannot commit; conflicting request-ID bindings are refused; no incomplete destination becomes Ready.
- [ ] Whole-Cell thread/operation/grant retirement is observed on 2 CPUs; missing tracked acknowledgements quarantine charged pages. Untracked exports/shared-lock invariants cannot yield a successful restart or forced unlock; no Drop-only cleanup assumption.
- [ ] SAS/shared-root and Domain/private-root cases use incarnation/object identity rather than physical-address inequality.
- [ ] Ten cycles reconcile released vs explicitly charged quarantine state, with zero unknowns for any no-leak claim.
- [ ] 09C proves fresh authorized device lease and readback after 08; any durable claim requires the named failure-class evidence.

## Assumptions

Actual transfer/publication rights and state-domain identity representation require source/consumer verification; do not assume monotonic Cell generations implement them. Resource counters must be source-owned and complete before qualifying accounting.

## Security and risk assessment

Names and LBI do not authorize state transfer. A bad ownership retrofit can block legitimate recovery or expose state to a reused ID. Undo with known-good image plus cold boot; committed external effects and new persistent fencing epochs are not undone by binary revert. No persistent format is changed by this planning document.

## Deviation Log

SAS review: explicit destination-bound transfer replaces CellId-equality/generation-order rules; split early authority fix, software recovery and device extension; removed fresh-address assertions and volatile dedup shortcut.
