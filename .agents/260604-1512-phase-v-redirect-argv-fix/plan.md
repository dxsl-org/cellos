---
title: "Phase V — Append/Stdin Redirect + Per-Spawn ARGV Fix"
description: "Wire up >> and < shell redirects (helpers already exist) and fix ARGV stash race with per-task personal keys (kernel-only)."
status: pending
priority: P2
effort: 2h
branch: main
tags: [shell, kernel, syscall, redirect, argv, state-stash]
created: 2026-06-04
---

# Phase V — Redirect Wiring + Per-Spawn ARGV Fix

Two independent, low-risk fixes. Both call existing, verified helpers — no new
ABI surface (no `libs/api`, no `ViSyscall` changes → no Law 1 confirmation).

## Phases

| # | Phase | Status | Effort | File Owner |
|---|-------|--------|--------|------------|
| 1 | Redirect wiring (`>>`, `<`) + ARGV per-spawn key | pending | 2h | see below |

Single phase, two features touching two non-overlapping files:
- **Feature 1** — `cells/apps/shell/src/executor.rs` only (helpers in `cmd_fs.rs` already exist, unchanged)
- **Feature 2** — `kernel/src/task/syscall.rs` only (`state_stash.rs` unchanged)

No file is touched by both features → safe to land in one phase.

## Verified Facts (re-grepped 2026-06-04)

- `cmd_fs::append_file` — cmd_fs.rs:305 (exists, unused by executor)
- `cmd_fs::read_file_vfs` — cmd_fs.rs:351 (exists, returns byte count)
- `commands::cmd_echo_to_vec` — commands.rs:265 (exists)
- echo `StdoutTo` capture block — executor.rs:362-374
- combined `StdoutTo | StdoutAppend` stub — executor.rs:386-390 (the bug)
- `StdinFrom` print stub — executor.rs:380-385
- `SpawnFromPath` handler — syscall.rs:795-815; calls `loader::spawn_from_path` → `ViResult<usize>` (loader.rs:44), the `usize` is the new task id, propagated as `Ok(task_id)`
- `StateRestore` handler — syscall.rs:1086-1091
- `state_stash::{stash,restore}` — state_stash.rs:30,44; `restore` LEAVES entry in place (no eviction), `stash` REJECTS new keys when `MAX_ENTRIES=64` full (state_stash.rs:32)
- `ARGV_STASH_KEY = 0x0061_7267_7600_0000` — libs/ostd/src/syscall.rs

## Key Risk (must mitigate in Phase 1)

Personal keys `ARGV_KEY ^ (task_id << 32)` are stashed but NEVER removed
(`restore` does not evict). After 64 distinct spawns the stash hits
`MAX_ENTRIES` and silently rejects new argv → spawned cells get stale/empty
args. **Mitigation: in `SpawnFromPath`, reuse the global ARGV slot as scratch
(overwrite, not new key) OR cap personal keys by clearing on restore.**
See phase-01 Step B for the chosen approach.

## Dependencies

None external. Feature 2 depends on no Feature 1 work. Both buildable
independently; tested together.

## Success Criteria

- `echo A > /tmp/t; echo B >> /tmp/t; vcat /tmp/t` prints A then B.
- `cat < /tmp/t` (or `vcat < ...`) prints file content (no `[redir < ]` stub).
- Rapid sequential spawn test: shell spawns 2 cells back-to-back with
  different args; each reads its OWN args. Existing 52 tests still green.

## Detail

See [phase-01-implementation.md](./phase-01-implementation.md).
