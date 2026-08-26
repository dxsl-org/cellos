# Phase 01 — Redirect Wiring + Per-Spawn ARGV Fix

**Priority:** P2  **Status:** pending  **Effort:** ~2h

## Context Links

- Plan overview: [plan.md](./plan.md)
- Edit targets:
  - `cells/apps/shell/src/executor.rs` (Feature 1)
  - `kernel/src/task/syscall.rs` (Feature 2)
- Unchanged helpers relied upon:
  - `cells/apps/shell/src/cmd_fs.rs` — `append_file:305`, `read_file_vfs:351`
  - `cells/apps/shell/src/commands.rs` — `cmd_echo_to_vec:265`
  - `kernel/src/cell/state_stash.rs` — `stash:30`, `restore:44`

## Overview

Two independent fixes. All called helpers already exist and are verified.
No `libs/api` / `libs/types` / `ViSyscall` changes → no Law 1 confirmation.

---

## Feature 1 — `>>` append and `<` stdin redirects

### Data flow

`echo A >> /tmp/t` → parser yields `Cmd{argv:["echo","A"], redirects:[StdoutAppend("/tmp/t")]}`
→ exec_cmd detects echo+StdoutAppend → `cmd_echo_to_vec(args)` → `append_file(path, bytes)`
→ VFS cell OP_APPEND → reply byte → return 0.

`cat < /tmp/t` → `Cmd{argv:["cat"], redirects:[StdinFrom("/tmp/t")]}`
→ exec_cmd redirect loop reads file via `read_file_vfs` → prints content directly
→ then continues to dispatch (cat with no path is a no-op / harmless).

### Current bug (verified)

executor.rs:386-390 — `StdoutTo(path) | StdoutAppend(path)` share ONE arm that
only prints `[redir > path]`. The echo capture block at 362-374 handles only
`StdoutTo`. So `>>` falls through to the stub and never writes.
executor.rs:380-385 — `StdinFrom` only prints `[redir < path]`.

### Implementation Steps

**Step 1.1 — add echo append-capture block.**
RIGHT AFTER the existing `StdoutTo` echo block (after executor.rs:374), insert:

```rust
    // Phase V: capture `echo` output for `>>` append redirect.
    if prog == "echo" {
        if let Some(Redirect::StdoutAppend(path)) =
            cmd.redirects.iter().find(|r| matches!(r, Redirect::StdoutAppend(_)))
        {
            let bytes = crate::commands::cmd_echo_to_vec(&args);
            if !crate::cmd_fs::append_file(path, &bytes) {
                ostd::io::print("echo: cannot append '");
                ostd::io::print(path);
                ostd::io::println("'");
            }
            return 0;
        }
    }
```

**Step 1.2 — split the combined stub arm + wire StdinFrom.**
Replace the redirect `for` loop body (executor.rs:378-397). Change:
- `StdinFrom(path)` from print-stub → read file and print content.
- separate `StdoutAppend` out of the `StdoutTo | StdoutAppend` arm so a
  non-echo `>>` no longer prints a misleading `[redir >]` AND echo `>>` is
  already handled above (won't reach here).

```rust
    for r in &cmd.redirects {
        match r {
            Redirect::StdinFrom(path) => {
                // Phase V: load file into a buffer and emit it as the command's
                // input. Full stdin plumbing through dispatch is deferred; for the
                // common `cat < file` case, printing the content here is sufficient
                // and matches `vcat file` semantics.
                let mut buf = alloc::vec![0u8; 4096];
                let n = crate::cmd_fs::read_file_vfs(path, &mut buf);
                if n > 0 {
                    if let Ok(s) = core::str::from_utf8(&buf[..n]) {
                        ostd::io::print(s);
                    }
                } else {
                    ostd::io::print("shell: cannot open '");
                    ostd::io::print(path);
                    ostd::io::println("'");
                }
            }
            Redirect::StdoutTo(path) => {
                // Non-echo stdout redirect: external capture needs pipe caps (Phase 17a).
                ostd::io::print("[redir > ");
                ostd::io::print(path);
                ostd::io::println("]");
            }
            Redirect::StdoutAppend(path) => {
                // Non-echo append redirect: same Phase 17a limitation.
                ostd::io::print("[redir >> ");
                ostd::io::print(path);
                ostd::io::println("]");
            }
            Redirect::StderrTo(path) => {
                ostd::io::print("[redir 2> ");
                ostd::io::print(path);
                ostd::io::println("]");
            }
        }
    }
```

> Note: when `StdinFrom` prints content above and then dispatch runs `cat`
> with no path argument, `cat` is a harmless no-op. This is the KISS Phase-V
> scope; threading the buffer into `dispatch_builtin` is out of scope (YAGNI).

### Feature 1 edge cases

- `echo X > f` (single `>`) — unchanged, still hits block at 362-374.
- `echo X >> f` on missing file — `append_file` should create-or-append; if VFS
  OP_APPEND requires existing file, verify behavior (UNRESOLVED Q1).
- `< nonexistent` — prints `cannot open` (n==0 branch).
- Both `>` and `>>` on one command — echo blocks check `>` first, return early.

---

## Feature 2 — Per-spawn ARGV personal key (kernel-only)

### Root cause (verified)

`spawn_external` (executor.rs:604) calls `sys_set_spawn_args(...)` then
`sys_spawn_from_path(...)`. Args land in the single global slot `ARGV_STASH_KEY`.
If the shell spawns a second cell before the first is scheduled, the second
`set_spawn_args` overwrites the slot — first cell reads wrong args.

### Fix strategy

On spawn, kernel snapshots the pending global ARGV slot into a per-task
personal key derived from the new task id. On restore, a caller asking for
`ARGV_STASH_KEY` is served its personal key first.

### Critical risk + mitigation

`restore` does NOT evict (state_stash.rs:44-50) and `stash` rejects NEW keys
once `MAX_ENTRIES=64` is reached (state_stash.rs:32). Unbounded personal keys
would exhaust slots after 64 spawns → silent arg loss.

**Mitigation (chosen):** in `StateRestore`, after serving the personal key,
the personal entry stays (restore can't evict from the public API). To bound
growth WITHOUT touching state_stash.rs API, the `SpawnFromPath` handler
CLEARS the personal slot by overwriting it with empty bytes on the NEXT spawn
of the same task id is not viable (ids differ). Instead: **add a small
`remove(key)` to state_stash.rs and call it from `StateRestore` after a
successful personal-key read** (consume-on-read for argv only). This keeps the
generic stash semantics intact (leave-in-place) for hot-swap keys, while argv
personal keys are one-shot. This is the only state_stash.rs change and it is
additive (new `pub fn remove`), no ABI impact.

### Implementation Steps

**Step 2.1 — add `remove` to state_stash.rs** (after `restore`, ~line 50):

```rust
/// Remove and discard the entry for `key`, freeing its slot. Used for one-shot
/// keys (e.g. per-spawn argv) so personal keys do not accumulate toward
/// `MAX_ENTRIES`. No-op if `key` is absent.
pub fn remove(key: u64) {
    STASH.lock().remove(&key);
}
```

**Step 2.2 — snapshot argv on spawn.** In `SpawnFromPath` (syscall.rs:810-814),
bind the task id and transfer the global slot to a personal key:

```rust
            let task_id = crate::loader::spawn_from_path(path_str).map_err(|e| match e {
                types::ViError::NotFound => SyscallError::FileNotFound,
                types::ViError::OutOfMemory => SyscallError::Unknown,
                _ => SyscallError::InvalidInput,
            })?;
            // Phase V: transfer pending spawn args to a per-task personal slot so
            // a subsequent spawn overwriting the global slot cannot race this cell.
            const ARGV_KEY: u64 = 0x0061_7267_7600_0000; // = ostd ARGV_STASH_KEY
            let mut argv_buf = [0u8; 512];
            let n = crate::cell::state_stash::restore(ARGV_KEY, &mut argv_buf);
            if n > 0 {
                let personal_key = ARGV_KEY ^ ((task_id as u64) << 32);
                crate::cell::state_stash::stash(personal_key, &argv_buf[..n]);
            }
            Ok(task_id)
```

> The original arm was `crate::loader::spawn_from_path(path_str).map_err(...)`
> as the final expression. Rewrite it to bind `?` then return `Ok(task_id)`.

**Step 2.3 — serve + consume personal key on restore.** Replace the
`StateRestore` body (syscall.rs:1086-1091):

```rust
        Syscall::StateRestore { key, buf_ptr, buf_len } => {
            validate_user_buf(buf_ptr, buf_len, crate::cell::state_stash::MAX_STASH_LEN)?;
            // SAFETY: validated above — writable user buffer of exactly buf_len bytes.
            let buf = unsafe { core::slice::from_raw_parts_mut(buf_ptr as *mut u8, buf_len) };
            const ARGV_KEY: u64 = 0x0061_7267_7600_0000;
            if key as u64 == ARGV_KEY {
                // Serve this task's personal argv slot if present, then consume it
                // (one-shot) so personal keys never accumulate toward MAX_ENTRIES.
                let personal_key = ARGV_KEY ^ ((caller_id as u64) << 32);
                let n = crate::cell::state_stash::restore(personal_key, buf);
                if n > 0 {
                    crate::cell::state_stash::remove(personal_key);
                    return Ok(n);
                }
            }
            Ok(crate::cell::state_stash::restore(key as u64, buf))
        }
```

### Feature 2 trace (control flow)

1. Shell task 8 → `set_spawn_args("L")` → global slot = "L".
2. Shell → `SpawnFromPath("/bin/lua")` → task 9 created; kernel copies "L" to
   `ARGV_KEY ^ (9<<32)`.
3. Shell → `set_spawn_args("P")` → global slot = "P" (overwrites; Lua already safe).
4. Shell → `SpawnFromPath("/bin/python")` → task 10; copies "P" to `ARGV_KEY ^ (10<<32)`.
5. Lua (task 9) → `StateRestore(ARGV_KEY)` → personal `^ (9<<32)` = "L" ✓, then removed.
6. Python (task 10) → `StateRestore(ARGV_KEY)` → personal `^ (10<<32)` = "P" ✓, then removed.

Global slot still holds last value as fallback for any cell that has no
personal key (backwards compatible with current single-spawn behavior).

### Backwards compatibility

- Cells using non-argv stash keys (hot-swap) hit the final
  `restore(key)` branch unchanged → leave-in-place semantics preserved.
- A cell spawned but reading ARGV when no personal key exists (e.g. spawned by
  a path that bypassed set_spawn_args) falls back to the global slot — same as
  today.

---

## Related Code Files

**Modify:**
- `cells/apps/shell/src/executor.rs` (Steps 1.1, 1.2)
- `kernel/src/task/syscall.rs` (Steps 2.2, 2.3)
- `kernel/src/cell/state_stash.rs` (Step 2.1 — additive `remove`)

**Create:** none. **Delete:** none.

## Todo List

- [ ] 1.1 Add echo `StdoutAppend` capture block after executor.rs:374
- [ ] 1.2 Split `StdoutTo | StdoutAppend` arm; wire `StdinFrom` to `read_file_vfs`
- [ ] 2.1 Add `pub fn remove(key)` to state_stash.rs
- [ ] 2.2 Bind `task_id` in `SpawnFromPath`; snapshot global ARGV → personal key
- [ ] 2.3 Serve+consume personal key in `StateRestore`
- [ ] `cargo check` (kernel + shell cell) — no errors
- [ ] `cargo clippy -- -D warnings`
- [ ] Boot QEMU; run redirect test sequence
- [ ] Run integration suite (expect 52 still green)

## Success Criteria

| Check | Observable result |
|-------|-------------------|
| Append | `echo A > /tmp/t; echo B >> /tmp/t; vcat /tmp/t` → `A` then `B` |
| Stdin | `cat < /tmp/t` (or `vcat < /tmp/t`) → file content, no `[redir <]` |
| ARGV race | Two back-to-back spawns w/ different args → each cell logs its own args |
| No regression | 52 existing integration tests pass |
| Slot bound | After >64 spawns, argv still delivered (personal keys consumed on read) |

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Personal keys exhaust MAX_ENTRIES (64) | Med | High | Step 2.1 `remove` consumes argv key on restore (one-shot) |
| `task_id` from `spawn_from_path` not the new cell's runtime id used by `caller_id` | Low | High | VERIFY Q2: confirm spawn return id == the id passed as `caller_id` when that task later calls StateRestore |
| XOR personal key collides with a real stash key | Very Low | Med | ARGV_KEY high bits are zero; `^ (id<<32)` stays in argv namespace; hot-swap keys use FNV name hashes (different range) |
| OP_APPEND on missing file fails | Low | Med | Q1: verify VFS append creates file or pre-create with `>` in test |
| `cat` no-op after StdinFrom print confuses output | Low | Low | Documented; Phase-V scope, acceptable |

## Security Considerations

- `validate_user_buf` already guards both stash buffers (syscall.rs:1081,1087) —
  unchanged.
- Personal key derivation uses only kernel-side `task_id`/`caller_id`; a cell
  cannot forge another cell's personal key (it can only request `ARGV_KEY`, and
  the kernel maps it to the CALLER's id).
- `remove` is kernel-internal; no new syscall surface.

## Next Steps / Follow-ups

- Full stdin plumbing through `dispatch_builtin` (thread buffer) — future phase.
- External-process stdout capture for non-echo `>` / `>>` — Phase 17a pipe caps.

## Unresolved Questions

- **Q1:** Does VFS `OP_APPEND` create the file when absent, or require it to
  exist? If create-on-append is unsupported, the append test must `> ` first
  (the given test already does `echo A > t` before `>> t`, so the happy path is
  covered — but document the missing-file behavior).
- **Q2:** Confirm the `usize` returned by `loader::spawn_from_path` is the same
  task id that will appear as `caller_id` when the spawned cell calls
  `StateRestore`. If the loader returns a CellId or a slot index instead of the
  scheduler task id, Step 2.2/2.3 keys won't match — trace `spawn_from_path` →
  scheduler `insert`/`current_task_id` before implementing.
