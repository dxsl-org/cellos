---
phase: 2
title: "Grant Ownership and Quota Integrity"
status: completed
priority: P1
effort: ""
dependencies: []
tier: thinking
---

# Phase 02: Grant Ownership and Quota Integrity

> Record Decision / Deviation / Surprise when encountered. Investigate quota before fixing; never promote a source hypothesis to a reproduced bug.

## Overview
Close the approved M1 ownership slice without adding a grant-transfer primitive or changing public ABI. Grant authority follows the allocating task; quota follows the correct allocation responsibility rather than accidental execution context.

## Requirements
- `GrantShare` gives access, not ownership. `into_raw` detaches a same-owner Rust wrapper, not kernel owner transfer.
- Kernel GrantFree checks owner TID. Same CellId or a Rust Send move is not evidence that another task can free it.
- Preserve owner generation, pin/quarantine, root retirement and IOMMU acknowledgement semantics, including recent driver-registration fixes.

## Architecture
Keep the current local owner handle and explicit existing share/lease APIs. Do not build a new shared-owner hierarchy.
`heap.rs:18-34` uses current-context alloc/dealloc. Scheduler watcher containers can outlive the allocating caller; existing IPC wire code supplies a kernel-attribution precedent for both allocation and Drop.
Quota mismatch was reproduced before the fix: subscribing context accounting changed `0 -> 400 -> 400` while the exit context remained `0 -> 0`.

## Assumptions
- Claim: watcher allocations are charged to a caller and deallocated in a different attribution context on current source. Confidence: medium. Reproduce subscriber/peer exit and inspect exact counters before edits.
- Claim: no supported consumer requires ownership transfer via GrantHandle::from_raw. Confidence: medium. Retry LSP references, else document unavailable pinned rust-analyzer and exhaust workspace/tools/tests text references. Preserve real into_raw use in GrantRegion.

## Related Files
- Modify: `libs/ostd/src/grant.rs`; affected callers only after reference inventory, including `cells/tests/vfs-test/src/grant_io.rs`.
- Conditional proven fix: `kernel/src/task/scheduler.rs`, `kernel/src/memory/cell_quota.rs`, minimal existing allocation-context helper location if needed.
- Read/preserve: `kernel/src/memory/heap.rs`, `kernel/src/task/ipc_wire.rs`, `kernel/src/task/pending_mailbox.rs`, `kernel/src/task.rs`, `kernel/src/task/syscall.rs` grant owner/reaper paths.
- Verify consumers: `cells/services/hypervisor/src/virtio_blk.rs`, `cells/tools/shell/src/commands.rs`.
- Regression home: existing kernel quota/retirement test-hook modules and host integration targets; extend them rather than introducing another test framework.
- Modify living docs/changelog through Main after runtime proof; no `libs/api/` or `libs/types/` edits.

## Implementation Steps
1. Inventory exported handle usages and trait assumptions; use LSP whenever available. Read current GrantFree/Share/owner-death paths and identify TID vs CellId semantics explicitly.
2. Correct GrantHandle documentation/examples and unsafe preconditions to allocating-task-local ownership. Preserve into_raw for same-owner wrapper handoff; from_raw may only reconstruct proven same-owner authority, never infer it from IPC or GrantShare.
3. Remove unsupported cross-task Send capability if it cannot satisfy owner-only Drop; do not introduce a shim or silently reassign ownership. Update affected in-tree callers to stay on the owning task, without widening kernel authority.
4. Exercise real allocation/share/holder access/release and sender death. Keep share/lease access distinct from owner deallocation. Verify no early free while pinned and no grantee-as-owner claim.
5. Before any allocator change, reproduce subscriber registration -> watched exit/reap and VFS watch lifecycle, sampling the allocating owner, peer and kernel contexts. Warm BTreeMap capacity separately and drain all pending lifecycle work before evaluating steady-state deltas.
6. If reproduced, apply existing kernel-attribution convention across every allocation and destruction of the identified kernel-owned containers, including transient vectors and cancellation branches. Preserve separately receiver-charged mailboxes; do not mark all syscall allocations kernel-owned.
7. Check the cost/DoS boundary: count limits or existing authority must still bound caller-triggered kernel bookkeeping. Moving charge to kernel must not turn an unprivileged unbounded subscription path into free memory consumption. If a new public quota contract is necessary, stop at the exact ABI checkpoint.
8. Make attribution restoration safe under the actual interrupt/preemption/hart model; do not leave temporary current-cell identity across a yield or context switch. Do not add headers to every allocation or switch allocator implementation for this bounded fix.
9. If the hypothesis is falsified, record the concrete counterexample/preventing guard and close only the investigation, retaining the grant-contract work. Never fabricate a quota patch to complete the phase.
10. Verify intended and hostile lifecycle cases on the fresh integrated kernel, with matched receiver accounting and recent driver/IOMMU regressions intact.

## Success Criteria
- [x] SDK no longer claims IPC/Share transfers owner; current same-owner wrappers and shell/hypervisor grant uses compile for RV64.
- [x] A receiver cannot claim owner-only free: kernel dispatch still requires the allocating owner TID. The 20 host pin/quarantine tests preserve exact lease, owner-death, acknowledgement and release behavior.
- [x] Counter-backed RV64 reproduction demonstrated attribution drift before the fix and `DEATH-SUBSCRIBER-QUOTA: PASS` after it.
- [x] Under 32 distinct pairs, eight duplicate registrations per pair, watched exit and watcher cancellation, caller and unrelated exit-context accounting stay `0 -> 0 -> 0` and `0 -> 0`.
- [x] Receiver-charged IPC code was not re-attributed; only scheduler-owned subscriber/pending-death storage uses Cell 0. Pair deduplication bounds repeated `NotifyOnExit`; terminal watchers cannot recreate queued storage. IOMMU/quarantine code is unchanged.
- [x] No new transfer syscall, capability class, public ABI, per-allocation header or global allocator rewrite.

## Security Considerations
Affine ownership is not a remote authorization token. Do not alter GrantFree owner checks to make SDK examples work. Global telemetry remains opt-in; test counter exposure stays test-only.

## Risk Assessment
Scoped revert of matched kernel/SDK/cell changes; never ship mixed ownership semantics. Preserve new reproduction evidence and regression defenses. Force-exit may skip Rust Drop: existing kernel retirement remains authoritative. No irreversible external changes.

## Deviation Log
- Reproduced: `DEATH_SUBSCRIBERS` growth was charged to the subscribing cell and later dropped under another execution context, leaving the subscriber at `400` bytes after drain.
- Decision: subscriber-map vectors and queued-death buffers are scheduler-owned. Allocate and destroy both under a non-yielding Cell-0 attribution guard; retain receiver charging for IPC mailboxes.
- Review correction: kernel funding required idempotent `(watched, watcher)` subscriptions, duplicate-suppressed pending events, Cell-0 destruction before zombie transfer, and terminal-watcher rejection to close self-watch/recursive-retirement requeue.
- Evidence: RV64 test-hook boot emits `[selftest] DEATH-SUBSCRIBER-QUOTA: PASS pairs=32 duplicates=8 owner 0->0->0 exit 0->0`; the final reviewer found no remaining actionable defect.
- Integrated evidence: `evidence/perf-results-phase02-reviewed/perf-local-20260905T092500Z-rv64-qemu-virt-2h-256m-v2-1.json` is `VALID` and completes the real two-hart `NotifyOnExit` benchmark lifecycle. Its target verdict remains independently `FAIL`; this phase does not reinterpret performance qualification.
- Verification: RV64 release builds pass with and without `test-hooks`; `ostd`, shell and bench grant consumers check for RV64; 20 host pin/quarantine tests pass; 16 benchmark comparator tests pass.
- Limitation: the broader VFS test-hooks image was not regenerated. Its PowerShell builder uses Windows path separators on Linux, the configured `riscv-none-elf-gcc` is absent, and the signing gate currently refuses an unrelated unallowlisted `driver-e1000` unsafe site. None is used as Phase-02 evidence.
