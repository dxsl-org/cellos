# Native development handoff r7 — N07 native and compatibility preparation

## Decision requested and current status

**DRAFT FOR REVIEW; NOT AUTHORIZED.** This packet prepares the exact source, caller migration, and verification boundaries for tranche **N07**. It does not authorize editing frozen syscall numbers, wire protocol renumbering, releasing locks, or promoting CP1/CP2. Existing scoped consents (Clusters 1–5), N02-A, N02-B, N03, N04, N05, and N06 implementation records remain unchanged; POSIX/C ABI promotions remain pending.

Approval of N07 requires an explicit user instruction naming both this Markdown candidate's SHA-256 and its companion JSON SHA-256, appended to `.agents/261011-1200-cell-contract-authority/native-development-handoff-decisions.jsonl`.

## Tranche scope: N07 native and compatibility: native stream mechanisms, POSIX/C adapter isolation, and bootstrap acyclicity

Integrating owner: controlling P01 owner. WIP: one tranche.
N07 establishes clean separation between native core primitives and optional compatibility adapters, enforcing neutral pipe stream mechanisms, client-local fd/errno/TLS state, and bootstrap acyclicity under D08/D09/D11.

### Exact allowed paths for N07 implementation (upon approval)

- `kernel/src/task/pipe.rs`: neutral bounded native stream mechanism, capacity clamping (64..65536 bytes), and endpoint token management.
- `kernel/src/task/syscall.rs`: native stream syscall arms (`PipeCreate`, `PipeRead`, `PipeWrite`, `PipeClose`, `PipeShare`) and POSIX fd adapter isolation.
- `kernel/src/task/tls.rs`: per-thread TLS base allocation and isolation across threads.
- `kernel/src/task/futex.rs`: neutral futex wait/wake primitives.
- `tests/unit_cspace_smoke.rs`: host harness tests for N07 stream bounds, fd isolation, and TLS mechanisms.
- `CHANGELOG.md`: bounded tranche record.
- `.agents/261011-1200-cell-contract-authority/validation.md`: append observed evidence.

Read-only consumers: `kernel/src/cell/cspace.rs`, `kernel/src/cell/cap_registry.rs`, `kernel/src/task/launch.rs`, `kernel/src/task/scheduler.rs`, `kernel/src/task/tcb.rs`, `kernel/src/task/async_ipc.rs`, `kernel/src/memory/cell_quota.rs`, `hal/arch/x86/src/x86_64/context.rs`, `libs/api/src/abi/syscall.rs`, `libs/api/src/abi/cell_owner.rs`, `kernel/src/loader.rs`, `kernel/src/policy.rs`.
Forbidden: new public syscall numbers, wire protocol renumbering, ambient global fd table creation, and SAS fallback on compatibility failure.

### Invariants and cutover requirements

1. **Preserve Neutral Core Primitives:** Keep pipe streams, TLS base, and futex wait/wake as neutral kernel mechanisms independent of POSIX, libc, or C runtime.
2. **Client-Local POSIX Adapter State:** Errno, TLS buffers, file descriptor tables, and allocator heaps remain strictly local to each client Cell; services receive typed resources, never client pointers to ABI objects.
3. **Bounded Native Stream Mechanisms:** Pipe creation writes exactly two endpoint tokens; capacity is clamped to a bounded range (64..65536 bytes); EOF and errors are clearly distinguished.
4. **Bootstrap Acyclicity:** Loader and VFS boot reads use minimal bootstrap reader without dependency cycles; direct kernel fd calls remain isolated.
5. **No Ambient Privilege or SAS Fallback:** Compatibility personalities remain subject to standard domain ceilings; no Tier-1 privilege escalation or raw-pointer fallback.

### Required verification and gates

- Host harness: verify stream buffer capacity bounds, pipe endpoint tokens, client-local fd isolation, and TLS base isolation.
- Kernel compilation: clean compile of `cellos-kernel` on `x86_64-unknown-none` (both default and `test-hooks`).
- Boot smoke: verify clean boot of `vicell-x86.iso` in VMware Workstation with native and compatibility paths intact.
- Document consistency: `python3 scripts/verify-cell-contract-p01.py --negative-controls` exits 0.

### External prerequisites remaining blocked

- Full POSIX personality profile and mlibc promotions remain pending.
- Hardware qualification (Intel VT-d/VMX, physical power-loss, bare-metal deployment) remains uncertified.
- Production issuer credentials and cluster placement remain unassigned.
