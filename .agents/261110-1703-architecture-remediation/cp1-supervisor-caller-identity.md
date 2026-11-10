# CP1 — supervisor authorises callers by measured image, not task name (Phase 06)

**Date:** 2026-11-10 · **Slice:** S2 (architecture remediation) · **Law 1:** confirmation 1 requested · **Finding:** architecture review C-1 (userspace half)

## 1. Problem

The kernel half of C-1 is fixed in Phase 06: launch edges, the raw power-off gate (502) and shell argv staging now key on the caller Cell's measured program name (`__ViCell_image`, verified signature) or the trusted-init latch, never on the task name.

`/bin/supervisor` still authorises `OP_HOTSWAP` (from `hotswap`) and operator `OP_POWER` (from `shutdown`) by reading the sender's **task name** through `GetProcs` (`cells/services/supervisor/src/main.rs:65,145,182-194`). A task name is the basename of the spawner-chosen path. A capability-free cell can be launched at `/home/x/shutdown` through the shell's sandbox edge, and its `OP_POWER` is then accepted as an operator reboot or poweroff.

The supervisor cannot learn the sender's measured image today:
- 441 `ResolveCellImage` answers only the registered VFS Cell (`kernel/src/task/cell-image.rs::resolve_for_caller`; record §9 of `docs/abi-boundary-records.md`).
- The caller trailer (`libs/api/src/abi/caller_identity.rs`) carries `(cell, generation, sender_tid, flags)` with no image name.

## 2. Options

| # | Change | ABI surface | Pros | Cons |
|---|---|---|---|---|
| **A (recommended)** | Widen who may call 441: the registered VFS Cell **or** a caller holding `SupervisorCap`. No byte change. | Record §9 "who may call" row (semantics of a frozen syscall) | Smallest change; the record and decoder already exist; the supervisor compares `identity.is_signed_program("shutdown")` exactly as VFS does for `sshd` | A second consumer learns which program another Cell runs (same information VFS already has) |
| B | Add a caller-trailer flag bit `CALLER_FLAG_POWER_OPERATOR` set by the kernel when the sender's image is signed `shutdown` (another for `hotswap`) | `caller_identity.rs` new flag bits (8 of 32 used) | No new caller of 441 | Kernel learns supervisor-specific roles; one bit per role, the anti-pattern the review flags (Medium "CapSet as service-rights carrier") |
| C | Keep name checks; document the risk | none | — | C-1 stays open for power/hotswap |

## 3. Proposal (option A) — exact delta

- `kernel/src/task/cell-image.rs::resolve_for_caller`: allow when the caller task belongs to the registered VFS Cell **or** holds `SupervisorCap`; otherwise `PermissionDenied` (unchanged).
- `docs/abi-boundary-records.md` §9, 441 row, "Authority" column: "registered VFS Cell or a `SupervisorCap` holder".
- `cells/services/supervisor`: resolve the sender's `(cell, generation)` from the attested caller trailer, call 441, accept `OP_POWER` only for `is_signed_program("shutdown")` and `OP_HOTSWAP` only for `is_signed_program("hotswap")`; the `GetProcs` name path is deleted. Add `ResolveCellImage` to its syscall allowlist.
- No change to the `CellImage` bytes, version, decoder, syscall number or allowlist bit.

## 4. Evidence planned for CP2

- Kernel host test: 441 answers the supervisor and VFS, refuses another caller.
- Supervisor host test: a sender whose image is unsigned or signed with another name is rejected for both opcodes; a sender signed `shutdown` is accepted.
- x86 `power-x86` integration lane (operator reboot/poweroff and power button) passes.
- Negative QEMU control if feasible: a sandbox cell at `/home/x/shutdown` gets `STATUS_REJECTED_CALLER`.

## 5. Rollback

Revert the three files; nothing persistent. Under the dev posture anyone holding the public dev seed can sign a binary named `shutdown`; the release posture closes that (same caveat as `/srv/sshd`).

## 6. Question to the owner

Approve option A (Law-1 confirmation 1), choose B or C, or defer.

## 7. Owner decision

2026-11-11: **option A approved** (Law-1 confirmation 1). CP2 follows the §4 evidence.
