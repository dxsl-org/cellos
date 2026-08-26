# Phase 04 — `read VAR` Builtin (X-4)

**Priority:** P2 | **Effort:** ~2h | **Status:** pending | **Files:** 1

## Context Links
- Builtin dispatch: `dispatch_builtin` (executor.rs:435 area; match arms 500-550)
- `set_var` (executor.rs:149)
- Stdin reader of record: `AsyncStdin::read_line` (async_utils.rs:11) uses
  `ostd::syscall::sys_read(0, &mut c)` at async_utils.rs:25
- `sys_read` signature: `sys_read(fd: usize, buffer: &mut [u8]) -> Result<usize, SyscallError>` (ostd/src/syscall.rs:294)

## Overview
Add a synchronous `read VAR` built-in that reads one line from stdin (fd 0) and
stores it in `VAR`. Mirrors `AsyncStdin::read_line` minus the async/history/ANSI
machinery — KISS: plain char loop until newline.

## Key Insights (verified correction to brief)
- The brief said use `sys_recv(INPUT_ENDPOINT=5)`. **Wrong.** The shell reads
  keystrokes via `sys_read(0, ..)` (fd 0 = stdin), confirmed at async_utils.rs:25.
  Use the SAME mechanism from the synchronous built-in.
- `dispatch_builtin` is synchronous and has no async context — a blocking
  char-poll loop is the correct fit (the REPL is the only reader; no contention).

## Architecture / Data Flow
`read VAR` → loop: `sys_read(0, &mut [u8;1])` → on byte: if `\r`/`\n` stop; else
push to a local `Vec<u8>` (cap ~127 to fit the 128-byte var value slot); on
`Ok(0)`/`Err` `sys_yield()` and retry → `set_var(VAR, line_str)`.

## Related Code Files
- Modify: `cells/apps/shell/src/executor.rs` — add `"read"` match arm + `cmd_read` fn

## Implementation Steps
1. Add a match arm in `dispatch_builtin` (alongside `unset` at 530):
   `"read" => cmd_read(args),`.
2. Implement `fn cmd_read(args: &[&str]) -> ViResult<()>`:
   - `let var = args.first().copied().unwrap_or("REPLY");` (POSIX default `$REPLY`).
   - `let mut line = Vec::<u8>::new();`
   - loop: `let mut c=[0u8;1];` match `ostd::syscall::sys_read(0,&mut c)`:
     `Ok(n) if n>0` → if `c[0]==b'\n'||c[0]==b'\r'` break; else if `line.len()<127`
     push; (optionally echo via `ostd::io::print` to mirror terminal behavior);
     `_` → `ostd::executor` not available in sync ctx → use `ostd::syscall::sys_yield()`
     then continue.
   - `if let Ok(s)=core::str::from_utf8(&line) { set_var(var, s); }`
   - `Ok(())`.
3. Handle backspace (0x08/0x7F) optionally for usability (pop + `\x08 \x08`).
4. Build shell, regenerate disk, boot, manual test (interactive — no auto test).

## Todo List
- [ ] `"read"` match arm
- [ ] `cmd_read` with sys_read(0,..) poll loop
- [ ] Default var `REPLY` when no arg
- [ ] 127-byte cap (fits VARS value slot)
- [ ] Optional echo + backspace
- [ ] Build + boot + manual test

## Success Criteria
- Script `echo Enter:; read LINE; echo Got:$LINE` — typing `hello`<Enter>
  prints `Got:hello`. (Manual/interactive — documented as no automated test.)
- `read` with no arg stores into `$REPLY`.

## Risk Assessment
| Risk | L×I | Mitigation |
|------|-----|------------|
| Busy-poll spins CPU while waiting | Med×Med | `sys_yield()` on empty read — same pattern as net tools |
| Line longer than 127 bytes truncated silently | Low×Low | Cap matches VARS slot; document truncation |
| Blocking read stalls REPL if no input source | Low×Med | Acceptable — `read` is inherently blocking; matches shell semantics |
| sys_read(0) re-enters while REPL also reads | Low×High | `read` runs INSIDE the REPL's command execution — REPL readline is not active concurrently; single reader invariant holds |

## Rollback
Remove the match arm + `cmd_read`. No persisted state.

## Security Considerations
Input is user keystrokes already trusted by the shell. UTF-8 validated before
storing; invalid sequences drop the line (no panic).

## Next Steps
Independent. Touches `executor.rs` — sequence after 02/03 to avoid diff conflict.
