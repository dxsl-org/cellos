# Phase 05 — cp / mv Fix (standalone util cells)

## Context Links
- `cells/apps/utils/src/bin/cp.rs` — current STUB (prints "arg-passing not yet wired")
- `cells/apps/utils/src/bin/mv.rs` — STUB (same)
- Arg API (VERIFIED): writer `sys_set_spawn_args(&str)` syscall.rs:619; reader
  `sys_spawn_args(&mut [u8]) -> usize` syscall.rs:625 (key `ARGV_STASH_KEY`).
- Spawner side: `spawn_external` calls `sys_set_spawn_args(&args.join(" "))` executor.rs:818.

## Overview
- **Priority:** P2
- **Status:** pending
- **Description:** Replace cp/mv stubs with real implementations that read argv via
  `sys_spawn_args`, copy src→dst through VFS (chunked), and (for mv) unlink src.
- **Flag:** SCOUT REPORT WAS WRONG about the arg API. It said
  `sys_state_restore("__shell_args", ...)`. The real API is
  `ostd::syscall::sys_spawn_args(&mut buf) -> usize`. Verify this links from a
  `#![no_std]` bin BEFORE writing the copy logic (it lives in ostd, so it should).

## Key Insights
- **Per-tid arg-stash required** (user decision): `cp a b & cp c d` races on the global
  `ARGV_STASH_KEY`. Fix: key the stash by the spawned task's TID so concurrent spawns
  don't clobber each other.
  - Discovery gate: check if `sys_state_stash(key: &str, data: &[u8])` accepts arbitrary
    keys (kernel StateStash in `libs/ostd/src/syscall.rs`). If yes, use `argv_{tid}` as
    key — no Law 1 change needed.
  - If stash only supports fixed keys: requires new `sys_set_spawn_args_for(tid, args)`
    syscall — ⚠️ Law 1 change. Confirm before implementing.
  - Cell reads its own TID via `ostd::syscall::sys_getpid()` (or equivalent), then
    reads stash at key `argv_{self_tid}`.
- No existing util reads spawn args yet — this phase establishes the per-tid read pattern.
- VFS `Write` content must fit ~440 B per IPC. Large files: `Write` first chunk, then
  `Append` subsequent chunks.
- These are separate cells (`#![no_std] #![no_main]`), NOT shell built-ins. They get
  `#![forbid(unsafe_code)]` per Law 4 (Cells = no unsafe).

## Architecture
```rust
// cp.rs
#![no_std]
#![no_main]
#![forbid(unsafe_code)]
extern crate ostd;

#[no_mangle]
pub fn main() {
    let mut argbuf = [0u8; 256];
    let n = ostd::syscall::sys_spawn_args(&mut argbuf);
    let args = core::str::from_utf8(&argbuf[..n]).unwrap_or("");
    let mut it = args.split_whitespace();
    let (src, dst) = (it.next().unwrap_or(""), it.next().unwrap_or(""));
    if src.is_empty() || dst.is_empty() {
        ostd::io::println("usage: cp <src> <dst>");
        ostd::syscall::sys_exit(1);
    }
    match copy_vfs(src, dst) {
        Ok(()) => ostd::syscall::sys_exit(0),
        Err(()) => { ostd::io::println("cp: copy failed"); ostd::syscall::sys_exit(1); }
    }
}

fn copy_vfs(src: &str, dst: &str) -> Result<(), ()> {
    // 1. read src fully (read_file_vfs into a sized buffer, or chunked read loop)
    // 2. write dst: first CHUNK(~400B) via VfsRequest::Write, rest via Append
}
```
`mv.rs`: identical arg-read + `copy_vfs(src,dst)`, then `VfsRequest::Unlink(src)`
(verify the exact unlink/remove variant name in api::ipc::VfsRequest).

DRY: factor `copy_vfs` + chunked-write into a shared module if both bins need it.
Per Law 5 (no mod.rs) and 200-line file limit, a small `cells/apps/utils/src/vfs_copy.rs`
shared by both bins is acceptable; otherwise duplicate the ~30 lines (KISS for 2 sites).

## Related Code Files
- MODIFY: `cells/apps/utils/src/bin/cp.rs`
- MODIFY: `cells/apps/utils/src/bin/mv.rs`
- OPTIONAL CREATE: `cells/apps/utils/src/vfs_copy.rs` (shared copy helper) — only if it
  doesn't force a `lib`/`mod` restructure; else inline.

## Implementation Steps
1. VERIFY: write a 3-line cp that reads `sys_spawn_args` and echoes it; build+boot; run
   `cp a b` and confirm "a b" prints. Confirms API + linkage. (Discovery gate.)
2. Find exact `VfsRequest::Unlink`/`Remove` variant in `libs/api/src/ipc.rs`.
3. Implement `copy_vfs` with read-then-chunked-write.
4. cp.rs: arg parse + copy + exit codes.
5. mv.rs: arg parse + copy + unlink + exit codes.
6. cargo build; boot; manual `cp /data/a.txt /data/b.txt; vcat /data/b.txt`.

## Todo
- [ ] DISCOVERY: confirm sys_spawn_args links + returns argv in a util bin
- [ ] Find VfsRequest unlink variant name
- [ ] copy_vfs read + chunked write
- [ ] cp.rs full impl + exit codes
- [ ] mv.rs full impl (copy + unlink) + exit codes
- [ ] Decide shared helper vs inline (DRY vs no-mod.rs)
- [ ] cargo build + boot + manual round-trip

## Success Criteria
- `cp /data/a.txt /data/b.txt; vcat /data/b.txt` → b.txt content == a.txt.
- `mv /data/b.txt /data/c.txt; vcat /data/c.txt` → moved; `vcat /data/b.txt` fails.
- Large file (> 440 B) copies correctly via chunking.
- Exit code 1 on missing args / failed copy.

## Risk Assessment
| Risk | L×I | Mitigation |
|------|-----|------------|
| `sys_spawn_args` doesn't link in no_std bin | L×H | Discovery step 1 gates everything else. It's in ostd::syscall, used by shell — should link. |
| Stash overwritten on concurrent cp/mv spawns | M×H | **USER DECISION**: per-tid stash key. Discovery gate: confirm StateStash supports arbitrary keys. If not → Law 1 new syscall required. |
| Write > 440 B truncates | M×H | Chunked Write+Append; test with a > 1 KB file. |
| mv unlink variant misnamed | L×M | Grep api::ipc::VfsRequest before coding. |
| Forbidding unsafe breaks a needed op | L×L | Cells must be safe (Law 4); copy is pure VFS IPC — no unsafe needed. |

## Security Considerations
- VFS read/write/unlink are capability-checked. cp/mv run with the spawner's caps.
- Arg buffer fixed at 256 B — long paths truncate; bound-check, don't overflow.

## Next Steps
- Independent of Phases 3,4,6 (disjoint files).
