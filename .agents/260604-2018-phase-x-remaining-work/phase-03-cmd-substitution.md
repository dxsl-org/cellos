# Phase 03 — `$(cmd)` Command Substitution (X-3)

**Priority:** P2 | **Effort:** ~4h | **Status:** pending | **Files:** 2

## Context Links
- `expand_token` (executor.rs:199-230) — where `$VAR`/`$?` expand
- `exec_cmd` (executor.rs:351) — dispatch entry; `dispatch_builtin` at 435
- Stub `capture_cmd` (executor.rs:341-346) — returns empty `Vec` today
- `cmd_echo_to_vec` (commands.rs:265) — already writes echo output to a `Vec`

## Overview
Implement `$(cmd)` for built-ins by routing built-in stdout into a static
capture buffer instead of UART, running the inner command, and substituting the
captured text. External binaries remain uncapturable (Phase 17a pipe-cap limit).

## Key Insights
- `ostd::io::print` writes UART directly — no fd indirection in built-ins, so a
  capture flag must gate the write at the call site.
- `cmd_echo_to_vec` already proves the buffer-output pattern; reuse it for echo.
- A static `CAPTURE_BUF` is single-shell-task safe (same `unsafe static` model
  as `VARS`), but is NOT re-entrant → forbid nested `$( $() )`.

## Architecture / Data Flow
`expand_token` sees `$(` → extract inner string up to matching `)` → set
`CAPTURE_MODE=true`, clear `CAPTURE_BUF` → `parse(inner)`+`execute` → built-ins
append to `CAPTURE_BUF` → set `CAPTURE_MODE=false` → trim trailing `\n` →
substitute UTF-8 text into the result string.

## Related Code Files
- Modify: `cells/apps/shell/src/executor.rs` — add `CAPTURE_MODE`/`CAPTURE_BUF`,
  `capture_into(bytes)` helper, replace stub `capture_cmd` (341), extend `expand_token`
- Modify: `cells/apps/shell/src/commands.rs` — make `cmd_echo` / `cmd_cat`-style
  output route through `capture_into` when capture is active (or reuse `cmd_echo_to_vec`)

## Implementation Steps
1. Add statics near `VARS`:
   `static mut CAPTURE_MODE: bool = false;`
   `static mut CAPTURE_BUF: Vec<u8> = Vec::new();` (const-init OK on modern Rust).
2. Add `fn capture_active() -> bool` and `fn capture_push(b: &[u8])` helpers in
   executor.rs (both `unsafe`-guarded, documented single-task).
3. Route built-in output: simplest KISS path — in `expand_token`'s `$()` branch
   use a dedicated `run_capture(inner: &str) -> String` that calls
   `cmd_echo_to_vec` / a small dispatch for the capturable set
   (`echo`, `pwd`, `vcat`/`cat`, `uptime`). Avoid threading capture through ALL
   of `dispatch_builtin` (YAGNI) unless the simple set proves insufficient.
4. In `expand_token`: when `bytes[i]==b'$'` and `bytes[i+1]==b'('`, scan to the
   matching `)` (single level only — bail to literal on nested `$(`), extract
   inner, `result.push_str(run_capture(inner).trim_end_matches('\n'))`, advance.
5. Build, regenerate disk, boot, test.

## Todo List
- [ ] CAPTURE_MODE / CAPTURE_BUF statics
- [ ] run_capture() for the built-in set (echo/pwd/vcat/uptime)
- [ ] `$(` detection + matching-paren scan in expand_token
- [ ] Trim trailing newline on substitution
- [ ] Reject/literal-pass nested `$( $() )`
- [ ] Build + boot + test

## Success Criteria
- `OUTPUT=$(echo CAPTURED_VALUE); echo $OUTPUT` → `CAPTURED_VALUE`.
- `echo "v=$(echo hi)"` → `v=hi` (mid-token substitution).
- `X=$(/bin/nc ...)` → empty string (documented: external uncapturable), no crash.

## Risk Assessment
| Risk | L×I | Mitigation |
|------|-----|------------|
| Nested `$( $() )` corrupts static buffer | Med×High | Detect inner `$(` during scan → pass through literally; document limit |
| Capture set drifts from real built-ins | Med×Med | Centralize capturable list in `run_capture`; one place to extend |
| Re-entrancy via function body calling `$()` | Low×High | Functions execute synchronously; `run_capture` is non-recursive for the built-in set — assert CAPTURE_MODE not already set, bail if so |
| Unmatched `)` | Low×Low | If no closing paren, treat `$(` as literal text |

## Rollback
Restore the `capture_cmd` stub, remove statics + `$()` branch. `commands.rs`
echo path reverts to direct print. No persisted state.

## Security Considerations
Captured bytes are command output already destined for the user's terminal — no
new exposure. Bound `CAPTURE_BUF` growth (cap at e.g. 4 KiB) to avoid a runaway
built-in exhausting heap.

## Next Steps
Depends on phase 02 ordering (same `expand_token`). Land after 02.
