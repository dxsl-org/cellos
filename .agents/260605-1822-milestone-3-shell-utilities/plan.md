---
title: "Milestone 3.1 + 3.2 — Enhanced Shell & Standard Utilities"
description: "Fix pipe/redirect capture, add tab-completion, find/uniq/top/kill, fix cp/mv."
status: pending
priority: P1
effort: 19h
branch: main
tags: [shell, utilities, vfs, ipc, no_std]
created: 2026-06-05
---

# Milestone 3.1 (Enhanced Shell) + 3.2 (Standard Utilities)

Parser already supports pipeline/redirect/control-flow/`$(...)`. The blocker is
runtime capture: `capture_cmd` is a stub returning `Vec::new()`, so every pipe and
non-echo redirect is dead. Phase 1 is the keystone fix.

## Verified ground truth (re-grepped 2026-06-05)
- `capture_cmd(cmd: &Cmd, _stdin: &[u8], jobs: &mut Jobs) -> Vec<u8>` — executor.rs:418 (STUB).
- `exec_pipeline` already calls `capture_cmd` per stage — executor.rs:400.
- All built-in output goes through `ostd::io::print/println` directly (must route through a sink).
- Redirects: `Redirect::{StdoutTo,StdoutAppend,StdinFrom,StderrTo}` — only `echo` honored (executor.rs:449-509).
- VFS write: `cmd_fs::write_file(path,&[u8])`, `append_file(path,&[u8])`, `read_file_vfs(path,&mut[u8])->usize` (cmd_fs.rs:260/266/300).
- Arg passing: writer `sys_set_spawn_args(&str)`, reader `sys_spawn_args(&mut[u8])->usize` via `ARGV_STASH_KEY` (syscall.rs:619/625). [Scout's `sys_state_restore("__shell_args")` is WRONG.]
- TAB lands in `AsyncStdin::read_line` escape state machine — async_utils.rs:11-99 (single fn).
- `ProcessInfo { id: usize, state: usize, name: [u8;32] }` — api/syscall.rs:231; `sys_get_procs(&mut [ProcessInfo])` — syscall.rs:507. No tick/CPU field.

## Phases

| # | Phase | Effort | Risk | Depends |
|---|-------|--------|------|---------|
| 1 | [OutputSink + capture_cmd fix](phase-01-output-sink-pipe-fix.md) | 4h | Med | — |
| 2 | [Built-in output migration](phase-02-builtin-output-migration.md) | 3h | Low (tedious, ~30 sites) | 1 |
| 3 | [Tab completion](phase-03-tab-completion.md) | 3h | Med | 2 |
| 4 | [find + uniq built-ins](phase-04-find-uniq-built-ins.md) | 2h | Low | 2 |
| 5 | [cp/mv fix](phase-05-cp-mv-fix.md) | 2h | Med (verify arg-stash API first) | 2 |
| 6 | [top + cooperative kill](phase-06-top-kill.md) | 2h | Med | 2 |
| 7 | [Integration tests](phase-07-integration-tests.md) | 3h | Low | 1-6 |

## Dependency graph

```
Phase 1 ──> Phase 2 ──┬──> Phase 3
                      ├──> Phase 4
                      ├──> Phase 5   (parallel; distinct files)
                      └──> Phase 6
Phases 1-6 ──> Phase 7
```

Phases 1→2 are strictly sequential (cannot migrate built-ins before the sink exists).
Phases 3,4,5,6 are mutually independent and touch disjoint files (safe to parallelize).

## File ownership (no two parallel phases share a file)
- P1: `executor.rs` (capture_cmd, redirect block) — sole owner of those regions.
- P2: `commands.rs`, `cmd_fs.rs`, `cmd_sys.rs` (print→shell_print swaps).
- P3: `async_utils.rs` (+ adds `BUILTINS` const reference).
- P4: `cmd_fs.rs` (new fns) + `executor.rs` dispatch (1-line additions).
- P5: `cells/apps/utils/src/bin/cp.rs`, `mv.rs` only.
- P6: `commands.rs` (new cmd_top/cmd_kill) + `executor.rs` dispatch.
- NOTE: P2/P4/P6 all touch `executor.rs` dispatch and `commands.rs`/`cmd_fs.rs`.
  Run P2 first (completes), THEN P4/P6 add isolated dispatch arms. Treat P4+P6
  dispatch edits as serialized against each other (same `match prog` block).

## Definition of done (milestone)
`ls /data | grep test | wc -l` prints a number; `ls /bin > /tmp/o; vcat /tmp/o` round-trips;
TAB completes commands; `find /data`, `uniq`, `cp`, `mv`, `top`, `kill` all functional;
all 7 integration scenarios pass under QEMU.

## User decisions (locked)
- P1: `UnsafeCell<OutputSink>` + `SinkGuard(impl Drop)` — approved
- P5: per-tid arg-stash key to prevent `cp a & cp b` race — approved (verify if StateStash supports arbitrary keys; if not → ⚠️ Law 1 new syscall required)
- P6: keep command name `kill`, document cooperative-only semantics clearly

## Unresolved questions
- P5: Does `sys_state_stash(key: &str, data)` accept arbitrary string keys?  
  If yes: use `argv_{tid}` at no API cost. If no: new `sys_set_spawn_args_for(tid, args)` → Law 1.
- P6: What is the exact shutdown opcode? Grep `Shutdown` in `libs/api/` before using `0xFF`.
