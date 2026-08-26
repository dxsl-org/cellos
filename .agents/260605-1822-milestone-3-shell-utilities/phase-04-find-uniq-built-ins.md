# Phase 04 — find + uniq Built-ins

## Context Links
- `cells/apps/shell/src/cmd_fs.rs` — VFS helpers: read_file_vfs:300, ListDir IPC pattern
- `cells/apps/shell/src/executor.rs` — dispatch_builtin:553 (add 2 arms)
- Depends on Phase 1+2 (uses `shell_print`/`shell_println`).

## Overview
- **Priority:** P2
- **Status:** pending
- **Description:** Add `find <dir> [-name pat]` (recursive listing) and `uniq [file]`
  (adjacent-line dedup, also pipe-aware via captured stdin) as shell built-ins.

## Key Insights
- `find` recurses via `VfsRequest::ListDir` whose reply caps at ~512 B (~30 entries).
  Dirs larger than that silently truncate — document, do not paper over.
- `uniq` must work both from a file arg AND from pipe stdin. In a pipeline, the stage's
  stdin arrives as the `_stdin: &[u8]` arg into `exec_cmd`/`capture_cmd`. Confirm how
  built-ins receive that stdin — currently `_stdin` is unused. `uniq`/`wc`/`grep` need
  it threaded through. (If stdin threading is missing, add it minimally for uniq:
  read `_stdin` when no file arg given.)
- `prev` line for dedup must be an owned `String` (not `&str` into the buffer) if the
  buffer is consumed line-by-line from a temporary — but `lines()` over a stable
  `&[u8]` slice yields `&str` valid for the loop, so `prev: &str` is fine here.

## Architecture
```rust
// cmd_fs.rs
pub fn cmd_find(dir: &str, pattern: Option<&str>) {
    find_recursive(dir, pattern, 0);
}
fn find_recursive(dir: &str, pattern: Option<&str>, depth: usize) {
    if depth > 16 { return; } // cycle/runaway guard
    // ListDir(dir) -> entries "d:name" / "f:name"
    for entry in list_dir(dir) {
        let (kind, name) = split_entry(&entry); // 'd' | 'f'
        let full = join_path(dir, name);
        match kind {
            'f' => if pattern.map_or(true, |p| name.contains(p)) {
                       crate::executor::shell_println(&full);
                   },
            'd' => { crate::executor::shell_println(&full);
                     find_recursive(&full, pattern, depth + 1); }
            _ => {}
        }
    }
}

pub fn cmd_uniq(file: Option<&str>, stdin: &[u8]) {
    let owned;
    let data: &[u8] = match file {
        Some(p) => { let mut b = alloc::vec![0u8; 4096];
                     let n = read_file_vfs(p, &mut b); b.truncate(n);
                     owned = b; &owned }
        None => stdin,
    };
    let mut prev: Option<&str> = None;
    for line in core::str::from_utf8(data).unwrap_or("").lines() {
        if prev != Some(line) { crate::executor::shell_println(line); }
        prev = Some(line);
    }
}
```

Dispatch arms (executor.rs, near line 567 with other fs cmds):
```rust
"find" => { let a = make_parts(args).collect::<Vec<_>>();
            let dir = a.first().copied().unwrap_or(".");
            let pat = a.iter().position(|&x| x=="-name").and_then(|i| a.get(i+1)).copied();
            crate::cmd_fs::cmd_find(dir, pat); Ok(()) }
"uniq" => { let f = args.first().copied();
            crate::cmd_fs::cmd_uniq(f, _stdin /* thread stdin in */); Ok(()) }
```
NOTE: `dispatch_builtin` currently has no `_stdin` param. To make `uniq` pipe-aware,
either (a) pass `_stdin` from `exec_cmd` into `dispatch_builtin`, or (b) accept
file-only `uniq` for v1 and defer pipe-stdin. Prefer (a) — small signature change,
benefits wc/grep too. **Decide and record in the phase PR.**

## Related Code Files
- MODIFY: `cells/apps/shell/src/cmd_fs.rs` (cmd_find, find_recursive, cmd_uniq, helpers)
- MODIFY: `cells/apps/shell/src/executor.rs` (2 dispatch arms; possibly thread `_stdin`)

## Implementation Steps
1. Add `list_dir`/`split_entry`/`join_path` helpers (or reuse cmd_ls's parsing).
2. Implement `cmd_find` + `find_recursive` with depth guard.
3. Implement `cmd_uniq` (file or stdin).
4. Decide stdin threading (a vs b); wire dispatch arms.
5. `cargo check` + manual: `find /data`, `find /data -name txt`, `cat f | uniq`.

## Todo
- [ ] list_dir/split_entry/join_path helpers (or reuse cmd_ls)
- [ ] cmd_find + recursion + depth guard (<=16)
- [ ] cmd_uniq (file + stdin paths)
- [ ] Decide & implement stdin threading for dispatch
- [ ] Register find + uniq in dispatch_builtin
- [ ] cargo check + manual tests

## Success Criteria
- `find /data` lists files/dirs recursively, one path per line.
- `find /data -name txt` lists only entries whose name contains `txt`.
- `cat dup.txt | uniq` collapses adjacent duplicate lines.
- `uniq /data/dup.txt` does the same from a file.

## Risk Assessment
| Risk | L×I | Mitigation |
|------|-----|------------|
| ListDir 512-B truncation hides files in big dirs | H×M | Document as known limitation; matches existing `ls` behavior. |
| Deep/cyclic dirs → stack overflow | L×H | depth guard (16) + no symlink following. |
| uniq stdin not threaded → only file mode works | M×M | Pick option (a); if deferred, document `uniq` as file-only for v1. |
| `-name` is substring not glob | L×L | Document; glob is YAGNI for now. |

## Security Considerations
- Read-only VFS ops; capability-checked. No write surface.

## Next Steps
- Independent of Phases 3,5,6. Serialize dispatch edits against Phase 6 (same match block).
