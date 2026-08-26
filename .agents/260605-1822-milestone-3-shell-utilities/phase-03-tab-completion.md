# Phase 03 — Tab Completion

## Context Links
- `cells/apps/shell/src/async_utils.rs` — `AsyncStdin::read_line`:11-99 (escape state machine)
- `cells/apps/shell/src/executor.rs` — dispatch_builtin:553 (canonical built-in list)
- VFS: `cmd_fs::read_file_vfs` shows the `VfsRequest::ListDir` IPC pattern (re-grep exact variant)

## Overview
- **Priority:** P2
- **Status:** pending
- **Description:** Handle TAB (`0x09`) in the `read_line` keypress loop. Complete the
  last whitespace token: against `BUILTINS` if no `/`, against VFS dir entries if it
  contains `/`.

## Key Insights
- TAB is NOT currently handled — in `read_line`, `0x09` falls through to "Echo and
  append" (async_utils.rs:87-91) and gets literally inserted into the buffer. Must add
  an explicit `if ch == 0x09 { ... continue; }` branch BEFORE the echo block, after the
  backspace branch (line 86).
- `read_line` owns `buffer: Vec<u8>` and prints incrementally. Completion must mutate
  `buffer` AND emit the delta to the terminal (no full cursor model exists here — the
  shell uses append + backspace only, so completion = print the missing suffix).
- No cursor-position tracking: completion only operates at end-of-line (the common case).
  Mid-line completion is out of scope (KISS).

## Architecture
Add a `BUILTINS` const (single source of truth). Place in `executor.rs` (next to
dispatch) and `pub` it, so dispatch and completion can't drift:
```rust
// executor.rs
pub const BUILTINS: &[&str] = &[
    "ls","cat","grep","wc","head","tail","echo","ps","help","mkdir","rmdir","rm",
    "vcat","vwrite","vappend","source","export","alias","unalias","jobs","kill",
    "find","uniq","top","cp","mv","exec","exit","read","sleep","clear","shutdown",
    "pwd","uname","env","uptime","free","test","unset","snapshot","blktest",
];
```
(Re-derive this list from the actual `match prog` arms in dispatch_builtin to avoid omissions.)

TAB branch in `read_line` (insert after backspace handling, ~line 86):
```rust
if ch == 0x09 { // TAB
    let line = core::str::from_utf8(&buffer).unwrap_or("");
    let token = line.rsplit(|c| c == ' ').next().unwrap_or(""); // last word
    if token.contains('/') {
        complete_path(token, &mut buffer); // ListDir IPC + filter
    } else {
        complete_builtin(token, &mut buffer);
    }
    continue;
}
```

`complete_builtin`: collect `BUILTINS` starting with `token`.
- 0 matches → no-op (optional bell).
- 1 match → print suffix `&m[token.len()..]`, push bytes to buffer.
- N matches → print `\n`, list candidates, reprint prompt + current buffer.

`complete_path`: split token at last `/` into (parent, prefix); send
`VfsRequest::ListDir(parent)`; entries are `d:name`/`f:name` (verify prefix format
against cmd_ls); filter by `prefix`; same 0/1/N behavior. Append `/` after a dir match.

## Related Code Files
- MODIFY: `cells/apps/shell/src/async_utils.rs` (TAB branch + 2 helpers)
- MODIFY: `cells/apps/shell/src/executor.rs` (add `pub const BUILTINS`)

## Implementation Steps
1. Add `pub const BUILTINS` to executor.rs (derive from dispatch arms).
2. Add TAB branch in `read_line` before the echo block.
3. Implement `complete_builtin(token, &mut Vec<u8>)`.
4. Implement `complete_path(token, &mut Vec<u8>)` reusing the ListDir IPC pattern.
5. Helper `reprint_line(prompt, buffer)` for the N-match case.
6. `cargo check` + manual TAB tests.

## Todo
- [ ] Add pub const BUILTINS to executor.rs
- [ ] TAB (0x09) branch in read_line (before echo/append)
- [ ] complete_builtin (0/1/N cases)
- [ ] complete_path via VfsRequest::ListDir + d:/f: parse
- [ ] reprint_line for ambiguous case
- [ ] cargo check + manual: `l<TAB>` -> `ls`, `cat /da<TAB>` -> `/data/`

## Success Criteria
- `l` + TAB completes to `ls ` (single match).
- `c` + TAB lists `cat cp clear ...` then reprints the line (multi match).
- `cat /da` + TAB completes the path segment from VFS.

## Risk Assessment
| Risk | L×I | Mitigation |
|------|-----|------------|
| BUILTINS list drifts from dispatch | M×L | Single `pub const`, derived from dispatch; add comment "keep in sync with dispatch_builtin". |
| ListDir 512-byte reply truncates large dirs | M×L | Accept; completion is best-effort. Document. |
| TAB inside escape sequence (0x09 after ESC) | L×L | TAB branch runs only when `escape_state==0`; place after the escape machine. |
| No cursor model → mid-line completion garbles | L×M | Restrict to end-of-line; documented limitation. |

## Security Considerations
- ListDir uses existing capability-checked VFS IPC. No new attack surface.

## Next Steps
- Independent of Phases 4-6.
