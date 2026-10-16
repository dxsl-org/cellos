# Native development handoff r6 — N06 replacement and recovery preparation

## Decision requested and current status

**DRAFT FOR REVIEW; NOT AUTHORIZED.** This packet prepares the exact source, caller migration, and verification boundaries for tranche **N06**. It does not authorize editing frozen syscall numbers, wire protocol renumbering, releasing locks, or promoting CP1/CP2. Existing scoped consents (Clusters 1–5), N02-A, N02-B, N03, N04, and N05 implementation records remain unchanged; durable genesis/claim/backend admissions remain pending.

Approval of N06 requires an explicit user instruction naming both this Markdown candidate's SHA-256 and its companion JSON SHA-256, appended to `.agents/261011-1200-cell-contract-authority/native-development-handoff-decisions.jsonl`.

## Tranche scope: N06 replacement and recovery: state/effect ownership, commit watermark, writer fence, and failure-class recovery

Integrating owner: controlling P01 owner. WIP: one tranche.
N06 establishes transactional service replacement and crash recovery within the kernel and supervisor, implementing unpublished preparation, writer epoch fencing, versioned state stash ownership, atomic commit watermarks, and creation-key publication.

### Exact allowed paths for N06 implementation (upon approval)

- `kernel/src/cell/state_stash.rs`: scoped/owned/versioned state stash with bounded allocation, epoch checking, and rejection of stale epochs.
- `kernel/src/cell/hotswap.rs`: prepare/restore/ready/publish lifecycle transitions and writer fencing.
- `kernel/src/cell/service_registry.rs`: service registration, replacement binding, and epoch tracking.
- `kernel/src/task/launch.rs`: stable creation-key registration and atomic publication rollback.
- `kernel/src/task/scheduler.rs`: hotswap/replacement publication synchronization under scheduler lock.
- `tests/unit_cspace_smoke.rs`: host harness tests for N06 state stash versioning, epoch fencing, and replacement lifecycle.
- `CHANGELOG.md`: bounded tranche record.
- `.agents/261011-1200-cell-contract-authority/validation.md`: append observed evidence.

Read-only consumers: `kernel/src/cell/cspace.rs`, `kernel/src/cell/cap_registry.rs`, `kernel/src/task/cap.rs`, `kernel/src/task/tcb.rs`, `kernel/src/task/async_ipc.rs`, `kernel/src/task/syscall.rs`, `kernel/src/memory/cell_quota.rs`, `hal/arch/x86/src/x86_64/context.rs`, `libs/api/src/abi/syscall.rs`, `libs/api/src/abi/cell_owner.rs`, `kernel/src/loader.rs`, `kernel/src/policy.rs`.
Forbidden: new public syscall numbers, wire protocol renumbering, anonymous unversioned state overwrites, and unapproved manifest v3 formats.

### Invariants and cutover requirements

1. **Prepare Unpublished & Writer Fencing:** Replacements are prepared unpublished. Outstanding mutations are drained and fenced; replacement cannot commit while the old writer is unfenced. Once fenced, the old writer epoch is invalidated and cannot commit.
2. **Versioned State Stash & Ownership:** State stash entries must be scoped to the exact service identity and generation; no anonymous overwrites or unbounded heap accumulation; stale epochs are rejected.
3. **Atomic Commit Watermark:** Restorations must catch up to the exact committed watermark before readiness publication. Publish occurs only after verified readiness.
4. **Stable Creation-Key Registration:** Keyed child registration under approved lifecycle ensures coordinator restarts query existing publications before recreating, preventing duplicate children in the same boot.
5. **No Stale Snapshot Publication:** Failure before fencing may retain old live service; failure after fencing keeps service unavailable until authority issues a fresh writer epoch. Never publish a partial or stale snapshot.

### Required verification and gates

- Host harness: verify versioned state stash storage, epoch fencing rejection of stale writers, atomic watermark catchup, and creation-key deduplication.
- Kernel compilation: clean compile of `cellos-kernel` on `x86_64-unknown-none` (both default and `test-hooks`).
- Boot smoke: verify clean boot of `vicell-x86.iso` in VMware Workstation with storage mounts and service registry intact.
- Document consistency: `python3 scripts/verify-cell-contract-p01.py --negative-controls` exits 0.

### External prerequisites remaining blocked

- Native durable execution backend and floor admissions remain pending.
- Hardware qualification (Intel VT-d/VMX, physical power-loss, bare-metal deployment) remains uncertified.
- Production issuer credentials and cluster placement remain unassigned.
