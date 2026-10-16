# Native development handoff r2 — N02-B shared authority and caller cutover preparation

## Decision requested and current status

**DRAFT FOR REVIEW; NOT AUTHORIZED.** This packet prepares the exact source, caller migration, and verification boundaries for tranche **N02-B**. It does not authorize editing frozen syscall interfaces, releasing locks, or promoting CP1/CP2. Existing scoped consents (Clusters 1–5) and N02-A implementation records remain unchanged; D05 and D07 remain unapproved.

Approval of N02-B requires an explicit user instruction naming both this Markdown candidate's SHA-256 and its companion JSON SHA-256, appended to `.agents/261011-1200-cell-contract-authority/native-development-handoff-decisions.jsonl`.

## Tranche scope: N02-B shared authority, scheduler publication convergence, and legacy caller cutover

Integrating owner: controlling P01 owner. WIP: one tranche.
N02-B replaces the split `CapSet`/TCB and `CapTable` capability models with single-source-of-truth CSpace authority under scheduler lock convergence, eliminating the legacy `CapTable` and its untracked global state.

### Exact allowed paths for N02-B implementation (upon approval)

- `kernel/src/cell/cspace.rs`: CSpace authority expansion, finite registry indexing, and caller interfaces.
- `kernel/src/cell/cap_registry.rs`: legacy allocator retirement and cutover to CSpace-backed file handles.
- `kernel/src/task/scheduler.rs`: transactional publication convergence and scheduler-locked authority resolution.
- `kernel/src/task/launch.rs`: atomic launch publication with complete rollback on CSpace insertion failure.
- `kernel/src/task/cap.rs`: unified `CapSet` snapshot and TCB capability representation convergence.
- `kernel/src/task/syscall.rs`: cutover of legacy `OpenCap`, `ReadCap`, `SeekCap`, `WriteCap`, `StatCap`, `TruncateCap`, `SyncCap`, `CloseCap`, and `CapRevoke` to CSpace authority.
- `kernel/src/task/cap_file_selftest.rs`: update selftest to verify CSpace-backed file handles.
- `tests/unit_cspace_smoke.rs`: host harness tests for N02-B shared authority, convergence, and cutover.
- `CHANGELOG.md`: bounded tranche record.
- `.agents/261011-1200-cell-contract-authority/validation.md`: append observed evidence.

Read-only consumers: `kernel/src/loader.rs`, `kernel/src/task/tcb.rs`, `kernel/src/policy.rs`.
Forbidden: new public syscall numbers, wire protocol renumbering, remote cluster wire activation, and unapproved manifest v3 formats.

### Invariants and cutover requirements

1. **Transactional Launch Publication:** `publish_live_cell_owner` must fail closed if CSpace authority allocation fails; no orphaned `Live` owner slot or runnable task may survive an insertion failure.
2. **Atomic RAII Allocation Context:** `launch.rs` must restore the caller's allocation attribution on all early return paths.
3. **Finite Registry Bounding:** authority accumulation across generations must be bounded; retired arenas must not exhaust memory unboundedly without explicit capacity refusal.
4. **Clean Legacy Cutover:** all eight file capability syscall arms must cut over simultaneously to CSpace `BootFSFile` slots. No dual-authority fallback or parallel registry is permitted.
5. **Exact CapRevoke Sequence:** maintain strict `teardown -> re-gate -> TCB/CSpace mutation -> best-effort notification` ordering to prevent ambient authority leaks.

### Required verification and gates

- Host harness: verify atomic launch failure rollback, clean file handle cutover, and bounded registry capacity refusal.
- Kernel compilation: clean compile of `cellos-kernel` on `x86_64-unknown-none` (both default and `test-hooks`).
- Boot smoke: verify clean boot of `vicell-x86.iso` in VMware Workstation with storage mounts intact.
- Document consistency: `python3 scripts/verify-cell-contract-p01.py --negative-controls` exits 0.

### External prerequisites remaining blocked

- D05 (VFS/NET handles) and D07 (thread/fault lifecycle) remain blocked pending full CP1 packages.
- Hardware qualification (Intel VT-d/VMX, physical power-loss, bare-metal deployment) remains uncertified.
- Production issuer credentials and cluster placement remain unassigned.
