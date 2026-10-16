---
phase: 3
title: "Multithreading, SMP lifetime and aggregate budgets"
status: pending
priority: P1
tier: thinking
dependencies: [2]
---

# P03 — Thread execution within one Cell

## Deliverable and requirements

AC-06..AC-09 and the local admission portion of AC-30. Threads share Cell identity/CSpace/resource ceilings, not TLS, stack or current receive context. Actual Intel x86 two-CPU execution is required; host interleaving tests alone do not prove context safety. P08 owns destination placement admission and service/broker capacity; P10 qualifies the integrated node/application bounds.

AC-48 is shared, like AC-30: P03 proves immutable maxima/current-limit distinction and local reserve/admit/retire primitives. P08-C supplies the real application Authority grant/claim integration and end-to-end resizing witness; P10 qualifies both. P03 completion must not wait for P08, close the entire AC-48 row, or enable application-managed growth using fixture grants. That profile remains closed until the P08 evidence gate; no reverse dependency is introduced.

AC-40 local ownership is limited to immutable-max/current-limit, aggregate accounting and reserve/admit/retire enforcement primitives. P08-C owns the application Authority vector, live destination claim and distributed shares; P03 neither approves their representation nor closes the distributed criterion.

## Related files / ownership

- `kernel/src/task/{scheduler.rs,tcb.rs,smp.rs,futex.rs,tls.rs}`, `kernel/src/task/hart_local/ready.rs`, `kernel/src/task.rs`, `kernel/src/memory/cell_quota.rs`.
- `hal/arch/x86/src/x86_64/{context.rs,syscall.rs}` and x86 interrupt/context paths located from those owners.
- `libs/ostd/src/task.rs`, `libs/port-platform/cellos_pthread.c`, runtime headers and TLS allocation consumers enumerated in P01.
- Runtime fixtures under `cells/tests/`; extend existing domain and pthread fixtures rather than another scheduler model.
- `scripts/build-x86_64-domain-test-ci.sh`, `scripts/x86/qemu-domain-test.sh`; adapt existing TLS/futex/pthread runners for qualified Intel images.

## Implementation steps

1. Join the live shared Cell record on thread creation; reserve total stack/thread resources, recheck not-Retiring at publish, and include every published member in root retirement. Retain current selected/current/executing/outgoing ownership fences; never enqueue a context still being saved elsewhere.
2. Preserve per-thread TLS base, distinct language TLS allocation, FPU/SIMD state and context stack through preemption/migration. Do not infer full `__thread` or hosted C++ support from SetTlsBase; implement/qualify exactly the approved native worker-pool profile.
3. Implement aggregate CPU quota from P01: sum execution time across CPUs, charge at switches/ticks, replenish once per period, bound overshoot by admitted CPUs/accounting interval. Saturate/refuse on counter overflow; constrain priority by Cell ceiling. Threads cannot multiply budget. Admit kernel/IRQ accounting exceptions explicitly; do not charge unrelated Cell runtime twice.
4. Preserve atomic futex compare-and-park and address-space/generation keys; cleanup waiters on worker/root exit. Specify wake/filter ownership and order across task migration and deadline sweep.
5. Apply target lifecycle policy: clean worker exit is local; root exit or unhandled shared-memory-threatening worker fault retires the entire Cell once. Emit cause and root/worker identity. Do not continue siblings with potentially corrupted mutex/heap state.
6. Close new admission, quiesce exact generation across all CPUs, wait for safe-root/retirement acknowledgements, then reclaim stacks, authority and quota. Missing CPU acknowledgement quarantines resources; no forced timeout release.
7. Adapt client/runtime expectations to whole-Cell fault scope and explicit joins. This changes semantics and must be in P01 delta approvals; do not silently rewrite an old test to expect a different policy.
8. Enforce P01's declared aggregate node admission bounds alongside per-Cell budgets. Reserve local Cell/thread/memory/CPU allocations before publication, roll back denied preparations, and preserve emergency/kernel reservations. Service-owned queue/socket/capacity limits remain enforced by their issuers and are integrated in P08; no fixed request capacity is inferred from CPU ceilings.
9. AC-48: keep immutable incarnation authority/resource maxima separate from current admitted allocation. Growth reserves its delta at independent Budget Authority and destination before use; above-maximum growth refuses and requires explicit budgeted replacement. Shrink below charged use remains pending until drain/reconciliation fits without losing admitted work. P08 supplies Authority management integration.

## Verification / success criteria

- [ ] Barrier-controlled spawn-versus-retire, context-save-versus-steal and wake-versus-park interleavings prove inclusion/refusal, no duplicate execution and no stale wake.
- [ ] x86 QEMU `X86_CPUS=2` domain lane positively shows CPU 1 executing, migration and remote acknowledgements.
- [ ] Two native C/C++ workers repeatedly check distinct TLS/errno and FPU values through preemption; exact mutex/condition counters and timeout/mismatch cases pass. Rust worker pool submits independent work without increasing Cell authority.
- [ ] Flood thread creation and parallel CPU work; total memory/cap/CPU budget stays within documented bound, unrelated Cell makes progress, priority cannot be raised beyond ceiling.
- [ ] Clean worker exit leaves sibling live; worker unhandled fault and root exit retire all members; no execution/frame reuse after retirement acknowledgement.
- [ ] AC-30 local saturation: several Cells below individual ceilings collectively reach node bounds; subsequent Cell/thread admission refuses before publication, accounting stays exact across failure/retirement, and released capacity becomes available only after confirmed reclamation.
- [ ] AC-48 local portion: immutable maxima cannot widen, above-max/missing local reservation growth refuses before use and below-use shrink stays pending until safe drain.

Correction 2026-10-16: prior checkmarks and cited standalone-model/storage-boot receipts did not establish these criteria. Acceptance remains pending; historical observations are not runtime qualification.

## Security, risk and rollback

Same-Cell threads are mutually trusted. Per-thread reply restrictions are not a security sandbox. Do not introduce priority donation as a hidden prerequisite; hard RT schedulability remains a separate measured claim. CPU budget precision depends on accounting interval; state a conservative bound rather than promise instantaneous preemption.

Rollback is a cold boot to known-safe scheduling/authority image; never live-migrate an old TCB layout or free quarantined frames to restore availability. Lost external effects from already running work are not reversible.

## Assumptions and deviation log

Current tests include deterministic two-hart kernel witnesses, but new shared authority/CPU quota and actual x86 language TLS/FPU behavior need fresh evidence. Runtime support gaps are implementation work in this phase, not a reason to mark AC-07 skipped. Deviation log: none.
