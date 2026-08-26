# Phase 01 — OutputSink + capture_cmd Fix (KEYSTONE)

## Context Links
- `cells/apps/shell/src/executor.rs` — capture_cmd:418, exec_pipeline:396, redirect block:447-509
- `cells/apps/shell/src/cmd_fs.rs` — write_file:260, append_file:266, read_file_vfs:300

## Overview
- **Priority:** P1 (highest — unblocks ALL piping and ALL non-echo redirection)
- **Status:** pending
- **Description:** Introduce a process-global `OutputSink` so command stdout can be
  redirected into an in-memory buffer. Replace the `capture_cmd` stub (returns empty)
  with real capture. Wire `>`/`>>` for every command, not just `echo`.

## Key Insights
- Shell is single-task: all commands run in one task context. `UnsafeCell<OutputSink>`
  (not `static mut`) is the approved pattern — makes interior-mutability intent explicit,
  same safety semantics. External cells never call `shell_print`, so concurrent-write
  is impossible. Nested pipelines (`echo $(ls | wc)`) need a RAII `SinkGuard` (Law 8)
  to restore the outer sink on all exit paths including panics.
- `exec_pipeline` (executor.rs:400) ALREADY calls `capture_cmd` per stage. Fixing the
  stub alone makes pipelines work — no pipeline-loop changes required.
- Built-ins currently call `ostd::io::print` directly; capture only works once they
  route through `shell_print` (that migration is Phase 2). **Phase 1 alone makes
  `echo`/pipelines partially work; full coverage needs Phase 2.**

## Data Flow
```
read_line -> parse -> execute -> exec_pipeline
   stage[i]: CURRENT_SINK = Buffer(&out); exec_cmd(...); CURRENT_SINK = Console
             out -> stdin_data for stage[i+1]
   last stage: write out to Console (or to VFS file if redirected)
```

## Architecture
Add to top of `executor.rs`:
```rust
use core::cell::UnsafeCell;

enum OutputSink { Console, Buffer(*mut alloc::vec::Vec<u8>) }

// UnsafeCell makes interior-mutability intent explicit vs. `static mut`.
// SAFETY invariant: only the single shell task ever reads/writes this.
static CURRENT_SINK: UnsafeCell<OutputSink> = UnsafeCell::new(OutputSink::Console);

/// Restores the previous OutputSink on all exit paths (including early returns).
/// Law 8: implements Drop for resource cleanup.
struct SinkGuard(OutputSink);
impl Drop for SinkGuard {
    fn drop(&mut self) {
        // SAFETY: single shell task; no concurrent access.
        unsafe { *CURRENT_SINK.get() = core::mem::replace(&mut self.0, OutputSink::Console); }
    }
}

/// All shell output MUST go through this (not ostd::io::print directly).
/// Error messages and the prompt MUST call ostd::io::print directly to bypass capture.
pub fn shell_print(s: &str) {
    // SAFETY: single shell task; no external cells call this function.
    match unsafe { &*CURRENT_SINK.get() } {
        OutputSink::Console   => ostd::io::print(s),
        OutputSink::Buffer(v) => unsafe { (**v).extend_from_slice(s.as_bytes()) },
    }
}
pub fn shell_println(s: &str) { shell_print(s); shell_print("\n"); }
```

Replace stub (executor.rs:418-423) — keep the real `&Cmd` signature:
```rust
fn capture_cmd(cmd: &Cmd, stdin: &[u8], jobs: &mut Jobs) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    // Save current sink and set buffer; SinkGuard restores on any exit path.
    let _guard = SinkGuard(unsafe {
        core::mem::replace(&mut *CURRENT_SINK.get(), OutputSink::Buffer(&mut out))
    });
    exec_cmd(cmd, stdin, jobs);
    // _guard.drop() restores previous sink here.
    out
}
```

Replace non-echo redirect placeholders (executor.rs:491-502). Use existing
`write_file`/`append_file` (they take `&[u8]`, VFS chunking is internal — verify
size limit, see Risk):
```rust
Redirect::StdoutTo(path) => {
    let mut cap: Vec<u8> = Vec::new();
    // SAFETY: single-task; restored immediately after.
    unsafe { CURRENT_SINK = OutputSink::Buffer(&mut cap as *mut _); }
    let code = dispatch_builtin(prog, &args, jobs);
    unsafe { CURRENT_SINK = OutputSink::Console; }
    set_var("?", i32_to_str(code));
    if !crate::cmd_fs::write_file(path, &cap) { /* err msg via shell_println */ }
    return code; // redirect consumes the command; skip the later dispatch
}
Redirect::StdoutAppend(path) => { /* same, append_file */ }
```
NOTE: restructure so a redirected command does NOT also fall through to the
final `dispatch_builtin` at line 512. The echo special-case already `return`s;
mirror that for the generic path.

## Related Code Files
- MODIFY: `cells/apps/shell/src/executor.rs` (only file in this phase)

## Implementation Steps
1. Add `OutputSink` enum + `CURRENT_SINK` static + `shell_print`/`shell_println` at top of executor.rs.
2. Rewrite `capture_cmd` to set Buffer sink, run `exec_cmd`, restore Console, return buffer.
3. Refactor `exec_cmd` redirect handling: route `StdoutTo`/`StdoutAppend` through a capture+VFS-write that `return`s, so the command is not double-executed.
4. Keep the existing `echo` fast-path OR delete it (now redundant once generic path works — prefer delete for DRY, but verify echo's argv handling matches dispatch first).
5. `cargo check -p shell` (or workspace) — fix borrow/lifetime errors on the raw pointer.

## Todo
- [ ] Add OutputSink enum + CURRENT_SINK + shell_print/shell_println
- [ ] Rewrite capture_cmd (real capture)
- [ ] Wire StdoutTo via capture + write_file, with early return
- [ ] Wire StdoutAppend via capture + append_file, with early return
- [ ] Resolve echo fast-path redundancy (keep or remove)
- [ ] cargo check passes
- [ ] Manual boot: `echo hi | wc -c` returns a number (after Phase 2 wc migration)

## Success Criteria
- `cargo check` clean.
- After Phase 2: `echo abc | wc -c` prints `4`; `echo abc > /tmp/x; vcat /tmp/x` prints `abc`.
- `capture_cmd` returns non-empty bytes for any built-in routed through `shell_print`.

## Risk Assessment
| Risk | L×I | Mitigation |
|------|-----|------------|
| `write_file` content > VFS IPC limit (~440 B) silently truncates | M×H | Confirm whether `write_file`/`append_file` already chunk internally. If NOT, add `vfs_write_chunked(path,&[u8],append)` (400-byte chunks: first Write, rest Append) and call it from the redirect path. |
| Double-execution of redirected command | M×M | Ensure redirect branch `return`s before line-512 dispatch. |
| Raw `*mut Vec` dangling if exec_cmd re-enters capture | L×H | Single-task, synchronous, restored before return — no re-entrancy across await. Document SAFETY. Nested pipelines reset/restore in LIFO order via stack-local `out`. |
| `shell_print` borrow of `CURRENT_SINK` while mutating | L×M | Read sink, copy raw ptr out, never hold `&CURRENT_SINK` across the write. |

## Security Considerations
- No new syscalls. VFS writes use existing capability-checked `VfsRequest`. Path comes
  from user input — same trust boundary as existing `vwrite`.

## Next Steps
- Phase 2 migrates built-ins to `shell_print` so their output is actually captured.
