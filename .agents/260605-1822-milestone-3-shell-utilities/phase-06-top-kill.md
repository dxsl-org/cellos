# Phase 06 — top + Cooperative kill

## Context Links
- `cells/apps/shell/src/commands.rs` — cmd_ps:230 uses `sys_get_procs(&mut [ProcessInfo;16])`
- `libs/api/src/syscall.rs:231` — `ProcessInfo { id: usize, state: usize, name: [u8;32] }`
- `libs/ostd/src/syscall.rs:507` — `sys_get_procs`; :368 — `sys_send(target, &[u8])`
- `cells/apps/shell/src/executor.rs` — dispatch_builtin:553 (add 2 arms)
- Depends on Phase 1+2 (`shell_print`/`shell_println`).

## Overview
- **Priority:** P2
- **Status:** pending
- **Description:** Add `top` (live process table refresh) and `kill <tid>` (cooperative
  shutdown IPC). Reuse the verified `cmd_ps` data path.

## Key Insights
- `ProcessInfo` has NO tick/CPU counter — `top` CANNOT show CPU%. Show PID/STATE/NAME
  only. Document this honestly; do not fabricate a percentage.
- `state` is a `usize` code: 0=Ready, 1=Running, 2=Waiting, 3=Terminated
  (api/syscall.rs:233). Map to strings.
- `kill` is COOPERATIVE: there is no `sys_kill` syscall. Send a shutdown message via
  `sys_send(tid, &[opcode])`. The target must handle it to exit; if it ignores the
  message, it keeps running. State this clearly to the user.
- VERIFY the shutdown opcode: scout guessed `0xFF`. Check whether a real
  `ShutdownRequest`/shutdown opcode constant exists in `libs/api` (grep `Shutdown`).
  If a defined constant exists, use it; do not invent `0xFF`.
- `top` refresh loop needs an exit path. Use non-blocking key check (`sys_read(0,..)`
  returning 0 when no input) + `sleep_ms`. Confirm `sys_read` on fd 0 is non-blocking
  in this shell context (read_line treats `Ok(0)` as "no data, yield" — async_utils.rs:93).

## Architecture
```rust
// commands.rs
pub fn cmd_top() {
    loop {
        crate::executor::shell_print("\x1b[2J\x1b[1;1H"); // clear + home
        crate::executor::shell_println("PID  STATE     NAME");
        crate::executor::shell_println("---  --------  ----------------");
        let mut buf = [api::syscall::ProcessInfo::default(); 16];
        if let Ok(n) = ostd::syscall::sys_get_procs(&mut buf) {
            for p in &buf[..n] {
                let name = cstr(&p.name); // trim at first 0
                // format "{:3}  {:8}  {}" — use a small no_std formatter
                line_print(p.id, state_str(p.state), name);
            }
        }
        crate::executor::shell_println("\n(press any key to exit)");
        // poll ~1s for a keypress; break on any byte
        if poll_key_for_ms(1000) { break; }
    }
    crate::executor::shell_print("\x1b[2J\x1b[1;1H");
}

fn state_str(s: usize) -> &'static str {
    match s { 0=>"Ready",1=>"Running",2=>"Waiting",3=>"Terminated",_=>"?" }
}

pub fn cmd_kill(tid_str: &str) {
    let tid: usize = parse_usize(tid_str).unwrap_or(0);
    if tid == 0 { crate::executor::shell_println("kill: invalid tid"); return; }
    let msg = [SHUTDOWN_OPCODE]; // from libs/api if it exists, else documented constant
    let _ = ostd::syscall::sys_send(tid, &msg);
    crate::executor::shell_println("kill: shutdown signal sent (cooperative)");
}
```

Dispatch arms (executor.rs):
```rust
"top"  => { crate::commands::cmd_top(); Ok(()) }
"kill" => { crate::commands::cmd_kill(args.first().copied().unwrap_or("")); Ok(()) }
```

## Related Code Files
- MODIFY: `cells/apps/shell/src/commands.rs` (cmd_top, cmd_kill, helpers)
- MODIFY: `cells/apps/shell/src/executor.rs` (2 dispatch arms)

## Implementation Steps
1. Grep `libs/api` for a shutdown opcode/`ShutdownRequest`; pick the real constant.
2. Implement `state_str`, `cstr` (name trim), small int formatter.
3. Implement `poll_key_for_ms` (loop sys_read fd0 + sleep, break on byte).
4. Implement `cmd_top` refresh loop.
5. Implement `cmd_kill` via `sys_send`.
6. Register both arms; cargo check; manual.

## Todo
- [ ] DISCOVERY: find real shutdown opcode in libs/api (else document the chosen one)
- [ ] state_str + name trim + int formatter helpers
- [ ] poll_key_for_ms (non-blocking fd0 + sleep)
- [ ] cmd_top refresh loop + clean exit
- [ ] cmd_kill via sys_send (cooperative)
- [ ] Register top + kill in dispatch
- [ ] cargo check + manual

## Success Criteria
- `top` shows a refreshing PID/STATE/NAME table; any key exits and clears the screen.
- `kill <tid>` of a cooperative cell removes it from subsequent `ps` output.
- `kill` of a non-cooperative cell prints "signal sent" and the target is unaffected
  (documented behavior, not a bug).

## Risk Assessment
| Risk | L×I | Mitigation |
|------|-----|------------|
| Invented `0xFF` opcode mismatches real protocol → kill silently no-ops | M×H | Discovery step 1: use real api constant; if none, define one + note targets must adopt it. |
| `sys_read(0)` blocks → top hangs forever | M×H | Confirm fd0 returns Ok(0) when empty (read_line relies on this). If blocking, gate top behind a documented "blocking" note or use a timed recv. |
| top in a pipeline (`top | cat`) loops into a buffer forever | L×M | Disallow/ignore redirect for `top`, or break immediately when sink != Console. |
| No CPU% misleads users | L×L | Header omits CPU; doc states ProcessInfo lacks tick data. |

## Security Considerations
- `sys_send` to an arbitrary tid — kernel must enforce send capability. A malicious
  shell user could spam shutdown msgs; cooperative model limits impact (targets choose
  to honor). Note for future: gate kill behind a capability check kernel-side.

## Next Steps
- Independent of Phases 3,4,5. Serialize dispatch + commands.rs edits against Phase 4
  (both touch dispatch match + a shared command module).
