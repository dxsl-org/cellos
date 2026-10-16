# Native development handoff r3 — N03 execution, lifetime authority, scheduler participant fence, and D07 fault funnel

## Decision requested and current status

**DRAFT FOR REVIEW; NOT AUTHORIZED.** This packet prepares the exact source, thread/worker lifecycle, scheduler participant fence, and fault funnel verification boundaries for tranche **N03**. It does not authorize editing frozen syscall interfaces, releasing locks, or promoting CP1/CP2. Existing scoped consents (Clusters 1–5), N02-A, and N02-B implementation records remain unchanged; D05 remains unapproved, and D07 fault lifecycle implementation is dependency-scoped to N03.

Approval of N03 requires an explicit user instruction naming both this Markdown candidate's SHA-256 and its companion JSON SHA-256, appended to `.agents/261011-1200-cell-contract-authority/native-development-handoff-decisions.jsonl`.

## Tranche scope: N03 execution, lifetime authority, scheduler participant fence, and D07 fault funnel

Integrating owner: controlling P01 owner. WIP: one tranche.
N03 establishes thread and Cell execution lifetime authority under scheduler lock convergence, implementing the exact D07 lifecycle boundary: distinguishing local clean-worker thread termination from whole-Cell fault funnel retirement, enforcing SMP context/participant retention fences, and applying aggregate Cell CPU quota bounding.

### Exact allowed paths for N03 implementation (upon approval)

- `kernel/src/task.rs`: Task exit funnel, root vs. worker distinction, and `terminate_current_cell_on_fault`.
- `kernel/src/task/scheduler.rs`: Scheduler retirement queue, deferred fault handling, SMP context switches, and participant retention fence.
- `kernel/src/task/syscall.rs`: Thread exit and lifecycle syscall handlers (`sys_exit`, `sys_force_exit`, `sys_notify_on_exit`).
- `kernel/src/task/completion.rs`: Operation and task completion lifetime coordination on thread exit.
- `kernel/src/task/cpu_accounting.rs`: Thread and Cell aggregate CPU quota accounting and entitlement enforcement.
- `kernel/src/task/launch.rs`: Worker thread spawn and root thread registration.
- `tests/unit_cspace_smoke.rs`: Host harness tests for N03 lifetime, D07 clean-worker vs. whole-cell retirement, and SMP context switches.
- `CHANGELOG.md`: Bounded tranche record.
- `.agents/261011-1200-cell-contract-authority/validation.md`: Append observed evidence.

Read-only source consumers:
- `kernel/src/cell/cspace.rs`: CSpace authority validation on task retirement.
- `kernel/src/task/tcb.rs`: Task control block layout.
- `kernel/src/policy.rs`: Security policy and fault actions.
- `kernel/src/loader.rs`: Image loader.
- `libs/api/src/abi.rs`: Syscall numbers and ABI types.

Forbidden: new public syscall numbers, wire protocol renumbering, remote cluster wire activation, and unapproved manifest v3 formats.

### Invariants and cutover requirements

1. **D07 Clean Worker vs Whole-Cell Invariant:** When a worker thread terminates cleanly via `sys_exit`, only that calling worker thread exits locally; its private stack and allocations are reclaimed, while the Cell's root task and sibling workers continue executing unimpeded. Unhandled faults must trigger the whole-Cell fault funnel (`terminate_current_cell_on_fault` -> `retire_deferred_fault`).
2. **SMP Context & Participant Retention:** Two-CPU cross-hart dispatch must preserve TLS and FPU state across switches. Outgoing participants must remain retained in scheduler structures until incoming state is committed on the target CPU.
3. **Aggregate CPU Quota Bounding:** Total CPU consumption across all worker threads within a Cell must be aggregated against the Cell's predeclared budget; unmetered worker thread creation or burst exploitation is rejected.
4. **Deferred Fault Quiescence:** Unhandled faults funneled to `Scheduler::retire_deferred_fault` must halt all pending and active threads for that cell without leaking allocations, dangling locks, or unretired tickets.
5. **No Ambient Lifecycle Authority:** Thread creation, exit, and notification cannot bypass CSpace and scheduler state locks.

### Required verification and gates

- Host harness: verify clean worker exit, whole-cell fault retirement, and CPU quota accounting in `tests/unit_cspace_smoke.rs`.
- Kernel compilation: clean compile of `cellos-kernel` on `x86_64-unknown-none` (both default and `test-hooks`).
- Boot smoke: verify clean boot of `vicell-x86.iso` in VMware Workstation and QEMU.
- Document consistency: `python3 scripts/verify-cell-contract-p01.py --negative-controls` exits 0.

### External prerequisites remaining blocked

- D05 (VFS/NET handles) remains blocked pending full CP1 packages.
- Hardware qualification (Intel VT-d/VMX, physical power-loss, bare-metal deployment) remains uncertified.
- Production issuer credentials and cluster placement remain unassigned.
