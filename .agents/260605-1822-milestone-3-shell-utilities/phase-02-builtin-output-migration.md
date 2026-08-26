# Phase 02 — Built-in Output Migration

## Context Links
- Depends on Phase 1 (`shell_print`/`shell_println` must exist in executor.rs).
- `cells/apps/shell/src/commands.rs`, `cmd_fs.rs`, `cmd_sys.rs`

## Overview
- **Priority:** P1
- **Status:** pending
- **Description:** Replace every direct `ostd::io::print(...)` / `ostd::io::println(...)`
  call inside output-producing built-ins with `shell_print(...)` / `shell_println(...)`.
  Until this lands, captured buffers from Phase 1 stay empty for non-echo commands.
- **Flag:** ~30 call-site edits. Low risk but tedious and mechanical.

## Key Insights
- Pure mechanical swap. Behavior is identical when sink == Console (the default at the
  REPL), so interactive output is unchanged; only piped/redirected paths gain content.
- Do NOT migrate `print_usize`/`print_*` helpers that bypass strings unless they feed
  user-visible output. Convert numeric output to a string and pass through `shell_print`
  (e.g. `wc -l` count). Audit each.
- Leave kernel-diagnostic prints (`[shell] ...` snapshot logs) on `ostd::io` — those are
  operator logs, not command stdout, and should never be captured into a pipe.

## Migration Targets (re-grep before editing — counts are estimates)
Run first: `grep -rn "ostd::io::print" cells/apps/shell/src/{commands,cmd_fs,cmd_sys}.rs`
- cmd_fs.rs: `cmd_ls` (also in commands.rs — verify), `cmd_cat`, `cmd_grep`, `cmd_wc`, `cmd_head`, `cmd_tail`, `cmd_vcat`
- commands.rs: `cmd_ps`, `cmd_echo`, `cmd_help`, `cmd_clear`(keep ANSI direct? it IS stdout → migrate)
- cmd_sys.rs: `cmd_uname`, `cmd_free`, `cmd_env`, `cmd_uptime`, `cmd_pwd`

Add to each module: `use crate::executor::{shell_print, shell_println};`

## Decision rule per call site
| Call site purpose | Action |
|-------------------|--------|
| Command stdout (data the user/pipe consumes) | → `shell_print`/`shell_println` |
| Numeric output (`print_usize` for wc count) | format to str → `shell_print` |
| Operator/diagnostic log (`[shell] ...`) | leave on `ostd::io` |
| Error message to user | → `shell_println` (so `2>` capture works later; for now Console) |

## Related Code Files
- MODIFY: `cells/apps/shell/src/commands.rs`
- MODIFY: `cells/apps/shell/src/cmd_fs.rs`
- MODIFY: `cells/apps/shell/src/cmd_sys.rs`

## Implementation Steps
1. `grep -rn "ostd::io::print" cells/apps/shell/src/` → enumerate exact lines.
2. Per module: add the `use` import.
3. Swap each data-output call per the decision rule above.
4. Convert `print_usize`/numeric helpers feeding stdout to `shell_print(&n_to_string)`.
5. `cargo check`.
6. Manual: `ls | wc -l`, `cat f | grep x`, `ps | grep shell`.

## Todo
- [ ] Enumerate all print sites (grep)
- [ ] Migrate cmd_fs.rs output sites
- [ ] Migrate commands.rs output sites
- [ ] Migrate cmd_sys.rs output sites
- [ ] Convert numeric (print_usize) stdout to shell_print
- [ ] Leave [shell] diagnostics on ostd::io
- [ ] cargo check clean

## Success Criteria
- `ls /data | grep test | wc -l` prints a line count (currently prints nothing).
- Interactive output (no pipe) visually unchanged.
- No remaining `ostd::io::print` in command-stdout paths (only diagnostics remain).

## Risk Assessment
| Risk | L×I | Mitigation |
|------|-----|------------|
| Miss a call site → that command's pipe output empty | M×M | Final grep audit; integration test per command. |
| Accidentally route a diagnostic into a pipe | L×L | Apply decision rule; `[...]`-prefixed logs stay on ostd::io. |
| `cmd_ls` defined twice (commands.rs + cmd_fs.rs) | L×M | Dispatch uses `commands::cmd_ls` (executor.rs:563). Migrate the one actually dispatched; confirm via grep. |

## Security Considerations
- None new. Pure output-routing change within the shell task.

## Next Steps
- Unblocks Phases 3-7 (they assume captured output works).
