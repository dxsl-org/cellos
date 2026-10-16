# Native development handoff r3 — N03 execution and lifetime preparation

## Decision requested and current status

**DRAFT FOR REVIEW; NOT AUTHORIZED.** This packet prepares the exact source, caller migration, and verification boundaries for tranche **N03**. It does not authorize editing frozen syscall interfaces, releasing locks, or promoting CP1/CP2. Existing scoped consents (Clusters 1–5), N02-A, and N02-B implementation records remain unchanged; D05 remains unapproved, and D07 wire/diagnostic carriers remain pending Law-1 CP1.

Approval of N03 requires an explicit user instruction naming both this Markdown candidate's SHA-256 and its companion JSON SHA-256, appended to `.agents/261011-1200-cell-contract-authority/native-development-handoff-decisions.jsonl`.

## Tranche scope: N03 execution and lifetime: multithreading, SMP lifetime, aggregate budgets, and D07 fault retirement

Integrating owner: controlling P01 owner. WIP: one tranche.
N03 establishes thread execution and lifecycle boundaries within one Cell under SMP, implementing the internal D07 fault funnel (clean worker exit is local; root exit or unhandled worker fault retires the whole Cell incarnation), SMP participant fencing, futex waiter cleanup, and aggregate CPU quota enforcement.

### Exact allowed paths for N03 implementation (upon approval)

- `kernel/src/task/scheduler.rs`: whole-Cell retirement on worker fault (`retire_deferred_fault`, `exit_task`), clean worker exit semantics, spawn rejection on `Retiring`, SMP participant tracking and retirement synchronization.
- `kernel/src/task/tcb.rs`: thread state, context tracking, root vs worker identification, per-thread TLS/FPU state invariants.
- `kernel/src/task/hart_local.rs`: trap-proven user fault provenance handoff and per-hart state.
- `kernel/src/task/smp.rs`: multi-hart cross-CPU switch completion and retirement fences.
- `kernel/src/task/futex.rs`: waiter cleanup on worker/root exit, generation key checking.
- `kernel/src/task/cpu_accounting.rs`: aggregate entitlement, period charge, burst bank, refusal/throttle.
- `kernel/src/task/retirement_selftest.rs`: verification of clean worker exit vs whole-cell fault retirement.
- `tests/unit_cspace_smoke.rs`: host harness tests for N03 thread lifecycle, SMP participant fences, and fault retirement convergence.
- `CHANGELOG.md`: bounded tranche record.
- `.agents/261011-1200-cell-contract-authority/validation.md`: append observed evidence.

Read-only consumers: `kernel/src/cell/cspace.rs`, `kernel/src/cell/cap_registry.rs`, `kernel/src/task/launch.rs`, `kernel/src/task/cap.rs`, `kernel/src/task/syscall.rs`, `kernel/src/memory/cell_quota.rs`, `hal/arch/x86/src/x86_64/context.rs`, `libs/api/src/abi/syscall.rs`, `libs/api/src/abi/cell_owner.rs`, `kernel/src/loader.rs`, `kernel/src/policy.rs`.
Forbidden: new public syscall numbers, wire protocol renumbering, public D07 diagnostic/event carrier deployment without CP1, and unapproved manifest v3 formats.

### Invariants and cutover requirements

1. **D07 Whole-Cell Fault Funnel:** An unhandled trap-proven user fault from ANY thread (worker or root) must route to the exact live owner `(CellId, generation, root_tid)` and trigger whole-incarnation retirement via the scheduler funnel. Siblings must not continue running when shared memory or cell integrity is compromised.
2. **Local Clean Worker Exit:** A clean thread exit (`Syscall::Exit`) by a worker thread (`tid != root_tid`) must remain strictly thread-local: release thread-owned resources and futex wait states upon quiescence, leaving the root thread, sibling threads, and Cell authority intact.
3. **Atomic Retiring Cutoff & Participant Fencing:** Transitioning a Cell to `Retiring` occurs under scheduler lock before member selection; all subsequent thread spawn or admission requests for that `(CellId, generation)` must fail closed. Quiescence and resource release must wait for all remote CPUs to complete outgoing context switches and TLB shootdowns; no early frame reuse.
4. **Aggregate CPU Quota & Bounded Overshoot:** Thread execution time must be aggregated across all CPUs at switches and timer ticks against the Cell's declared entitlement. Counter overflow must fail closed/saturate; priority cannot be raised beyond the Cell's ceiling.
5. **TLS & FPU State Isolation:** Threads within a Cell share CSpace authority and resource ceilings, but maintain strictly isolated TLS base pointers, FPU/SIMD state, and context stacks across migrations and preemptions.

### Required verification and gates

- Host harness: verify clean worker exit isolation, worker fault whole-cell retirement, spawn refusal during Retiring, futex waiter cleanup, and CPU accounting bounds.
- Kernel compilation: clean compile of `cellos-kernel` on `x86_64-unknown-none` (both default and `test-hooks`).
- Boot smoke: verify clean boot of `vicell-x86.iso` in VMware Workstation with SMP and storage mounts intact.
- Document consistency: `python3 scripts/verify-cell-contract-p01.py --negative-controls` exits 0.

### External prerequisites remaining blocked

- D05 (VFS/NET handles) remains blocked pending full CP1 packages.
- Public D07 event/evidence diagnostic carrier formats (152-byte/184-byte records) and new entrypoints remain design-proposed and unapproved for wire ABI.
- Hardware qualification (Intel VT-d/VMX, physical power-loss, bare-metal deployment) remains uncertified.
- Production issuer credentials and cluster placement remain unassigned.
