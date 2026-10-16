# Native development handoff r4 — N04 operations and waits preparation

## Decision requested and current status

**DRAFT FOR REVIEW; NOT AUTHORIZED.** This packet prepares the exact source, caller migration, and verification boundaries for tranche **N04**. It does not authorize editing frozen syscall numbers, wire protocol renumbering, releasing locks, or promoting CP1/CP2. Existing scoped consents (Clusters 1–5), N02-A, N02-B, and N03 implementation records remain unchanged; D04 wire/carrier changes remain pending Law-1 CP1.

Approval of N04 requires an explicit user instruction naming both this Markdown candidate's SHA-256 and its companion JSON SHA-256, appended to `.agents/261011-1200-cell-contract-authority/native-development-handoff-decisions.jsonl`.

## Tranche scope: N04 operations and waits: unified bounded operation lifecycle, multi-waiter completion, and D04/D10/D11 caller convergence

Integrating owner: controlling P01 owner. WIP: one tranche.
N04 establishes unified bounded operation and completion lifecycle within one Cell, implementing the internal D04 operation model (immutable original-owner lifetime, explicit same-Cell consumption handoff, single-winner take-versus-exit serialization, bounded multi-waiter registration, and Small-output retention).

### Exact allowed paths for N04 implementation (upon approval)

- `kernel/src/task/async_ipc.rs`: preallocated Cell-budgeted operation storage, lifecycle transitions (submit, reply, cancel, abandon, take), owner-thread vs consumption-handle handoff.
- `kernel/src/task/completion.rs`: multi-waiter queue, filtered waitset registration, bounded reservation.
- `kernel/src/task/completion_wait.rs`: scoped wait, OR readiness, wake-before-take synchronization.
- `kernel/src/task/waker.rs`: non-allocating completion append, waker notification ordering.
- `kernel/src/task/scheduler.rs`: owner-exit vs consuming-take serialization, operation abandonment on worker/root exit.
- `kernel/src/task/tcb.rs`: task async operation tracking, current receive context token capture.
- `kernel/src/task/syscall.rs`: cutover of legacy IPC operation syscall arms (`Submit`, `Take`, `Wait`, `Cancel`, `Current`, `Reply`).
- `tests/unit_cspace_smoke.rs`: host harness tests for N04 operation lifecycle, consumption handoff, and multi-waiter completion.
- `CHANGELOG.md`: bounded tranche record.
- `.agents/261011-1200-cell-contract-authority/validation.md`: append observed evidence.

Read-only consumers: `kernel/src/cell/cspace.rs`, `kernel/src/cell/cap_registry.rs`, `kernel/src/task/launch.rs`, `kernel/src/task/cap.rs`, `kernel/src/memory/cell_quota.rs`, `hal/arch/x86/src/x86_64/context.rs`, `libs/api/src/abi/syscall.rs`, `libs/api/src/abi/completion.rs`, `libs/api/src/abi/cell_owner.rs`, `kernel/src/loader.rs`, `kernel/src/policy.rs`.
Forbidden: new public syscall numbers, wire protocol renumbering, unapproved manifest v3 formats, and raw ring token bypass.

### Invariants and cutover requirements

1. **Provider Reply Handoff vs Consumption Handoff:** Inbound receive context captured via `current()` may be replied to by any live thread of the accepted provider Cell incarnation. Moving a consumption handle to a sibling thread transfers only active take/wait rights; the original owner-thread TID and lifetime remain immutable.
2. **Owner Exit vs Take Serialization:** Original-owner exit automatically abandons its operations whether or not consumption was handed off. Dispatched work terminalizes as uncertain; consuming take versus owner exit has exactly one winning locked transition.
3. **Scoped Multi-Waiter Readiness:** Waitset is an immutable bounded set of exact operation/event tickets registered by active consumer; OR readiness, one waiter registration per calling thread, no foreign wake or wildcard drain of another owner's results.
4. **Small-Output Preservation:** Undersized take returns `Small` with exact required bytes and retains terminal and all reservations; no payload copy or terminal corruption occurs on undersized buffer.
5. **Non-Allocating Completion/IRQ Append:** IRQ and leaf completion appends must not allocate memory or take the scheduler lock; reservation refusal rejects pre-dispatch without leaving accepted work.

### Required verification and gates

- Host harness: verify provider reply handoff, consumption transfer isolation, owner-exit vs take race serialization, Small-output retention, and multi-waiter completion dispatch.
- Kernel compilation: clean compile of `cellos-kernel` on `x86_64-unknown-none` (both default and `test-hooks`).
- Boot smoke: verify clean boot of `vicell-x86.iso` in VMware Workstation with storage mounts and async IPC intact.
- Document consistency: `python3 scripts/verify-cell-contract-p01.py --negative-controls` exits 0.

### External prerequisites remaining blocked

- D05 (VFS/NET handles) remains blocked pending full CP1 packages.
- Public D04 wire carrier formats and new syscall entrypoints remain pending Law-1 CP1.
- Hardware qualification (Intel VT-d/VMX, physical power-loss, bare-metal deployment) remains uncertified.
- Production issuer credentials and cluster placement remain unassigned.
