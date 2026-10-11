---
blockedBy: [.agents/261011-1200-cell-contract-authority/plan.md]
executionStatus: blocked
executionLock: CELL-CONTRACT-0024
lockScope: "Remaining Phase05 authority and async-read integration evidence"
---

# Milestone 2.1 — Complete VFS Service

> **CELL-CONTRACT-0024 — residual execution locked (2026-10-11).** Remaining Phase05 authority/async-read integration evidence must rebase with explicit owner handoff through the [Cell contract plan](../261011-1200-cell-contract-authority/plan.md). P01–P04 remain completed despite the older headline status. Existing evidence ceilings and ABI limits remain unchanged; documentation completion does not unlock execution. See the [inventory](../cell-contract-0024-locks.md).

**Status**: 📋 PLANNED  
**Priority**: P0  
**Target**: 2026-08-30  
**Effort**: ~3.5 weeks  
**Created**: 2026-06-05

---

## Goal

Complete the VFS service by wiring existing quota infrastructure, adding subdirectory listing, implementing capability-based access control, adding non-blocking async read, and creating a formal integration test suite.

---

## Scope Decision: What's "Complete VFS"

**Research findings (2026-06-05):**

| Original Goal | Decision | Rationale |
|--------------|----------|-----------|
| FAT32 support | **SKIP** — FAT16 is correct | FAT32 needs ≥65525 clusters (~256MB+); disk_v3.img is 40MB. `fatfs` crate auto-detects; no action needed until disk grows past 256MB |
| Directory creation/deletion/listing | **PARTIAL** — add subdirectory listing | mkdir/rmdir/unlink already work; missing: recursive listing beyond root level |
| File permissions (read/write/execute) | **Capability-based, not POSIX** | SAS OS: CellId + path prefix rules. No uid/gid/mode bits. No persistent storage on FAT |
| Async file operations (non-blocking) | **Two-opcode protocol** | `ReadAsync` → `PendingHandle` → `Poll`. No executor changes. Simplest viable path |
| Disk quota tracking | **Wire existing tracker** | `QuotaTracker` exists and works; just not called in write path |

---

## Phases

| # | File | Status | Effort | Description |
|---|------|--------|--------|-------------|
| 1 | [phase-01-quota-enforcement.md](phase-01-quota-enforcement.md) | ✅ DONE | 2 days | Wire QuotaTracker to write/append paths |
| 2 | [phase-02-directory-listing.md](phase-02-directory-listing.md) | ✅ DONE | 3 days | Recursive directory listing, stat on dirs |
| 3 | [phase-03-capability-access.md](phase-03-capability-access.md) | ✅ DONE | 4 days | CellId-based path access control |
| 4 | [phase-04-async-read.md](phase-04-async-read.md) | ✅ DONE | 5 days | Non-blocking ReadAsync + Poll opcodes |
| 5 | [phase-05-integration-tests.md](phase-05-integration-tests.md) | 🟡 IN PROGRESS | 3 days | vfs-test cell (8 scenarios); embed+CI test deferred |

---

## ⚠️ Status reconciliation (2026-06-06)

Code audit (`cargo check -p service-vfs` clean) found **phases 01–04 were already implemented**
in `cells/services/vfs/src/` (quota wiring, `list_fat16_dir`, `access.can_write`, ReadAsync/Poll)
— plan status was stale. `cells/apps/vfs-test/` also already existed with 7 scenarios
(lifecycle, dir ops, access Err3, **async ReadAsync/Poll**, ramfs, stat-dir, edge).

**Done this session (decoupled — source only, no kernel build):**
- Added `test-hooks` feature to `service-vfs` + `app-vfs-test` (cfg-gated 2 KiB quota — production stays 32 MB).
- Added scenario 8 `test_quota_limit` to vfs-test (writes past 2 KiB → `Err(2)`; release → re-write OK).
- `cargo check` clean for both crates × both feature states.

**B (host unit test) — NOT feasible:** `service-vfs` is `no_std`/`no_main` depending on `ostd`
(riscv asm) → `cargo test` on host cannot build it. Quota-limit logic is instead covered by the
e2e `test_quota_limit` (exercises the real `can_charge` in the Write path). A true unit test would
require extracting `QuotaTracker` into a host-testable lib (refactor — out of scope).

**Deferred (collides with never-die session's kernel build + shared `kernel/src/embedded/`):**
- Build `app-vfs-test` + `service-vfs` with `--features test-hooks`; embed into `kernel/src/embedded/vfs-test`.
- Add `include_bytes!("vfs-test")` to VFS `/bin` ([main.rs:31](../../cells/services/vfs/src/main.rs#L31)) + `update-embedded.ps1`.
- Add integration test in [boot.rs](../../tests/integration/tests/boot.rs): `spawn /bin/vfs-test` → wait `"ALL TESTS PASSED"`.
- Rebuild kernel, boot QEMU, verify all 8 scenarios PASS.
- → Run AFTER never-die/Phase 26 stabilizes (shared kernel artifact).

**Execution order**: 1 → 2 → 3 → 4 → 5. Phases 1-2 are independent; Phase 3 depends on understanding from Phase 1-2; Phase 4 is independent of Phase 3.

---

## Current State (2026-06-05)

| Component | Status |
|-----------|--------|
| FAT16 mount at `/data/` | ✅ Working |
| RamFS at `/tmp/` | ✅ Working |
| All 9 VfsRequest opcodes | ✅ Implemented |
| QuotaTracker struct | ✅ Exists in `quota.rs` |
| Quota enforcement in write path | ❌ Not wired |
| Subdirectory listing | ❌ `ListDir` only works at root level |
| File permissions | ❌ None — only path-prefix auth |
| Async/non-blocking reads | ❌ All synchronous |
| Integration tests | ❌ Manual shell tests only |
| FAT32 support | ✅ Not needed at current scale |

---

## Success Criteria

- [ ] Writing past quota returns `VfsResponse::Err` with quota-exceeded code
- [ ] `ListDir("/data/subdir")` correctly enumerates FAT16 subdirectory entries
- [ ] A Cell without write capability to `/data/` gets `PermissionDenied`
- [ ] `ReadAsync("/data/large.bin")` returns immediately with a handle; `Poll(handle)` returns data when ready
- [ ] Integration test suite: at least 10 test scenarios covering the above
- [ ] All 65 existing integration tests still pass
