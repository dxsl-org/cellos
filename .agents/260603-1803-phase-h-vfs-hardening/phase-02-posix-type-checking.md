# Phase 2: POSIX type checking on OP_RMDIR / OP_UNLINK

## Context links
- `cells/services/vfs/src/main.rs:312-322` — `unlink_fat16` (calls `remove`, accepts files AND dirs)
- `cells/services/vfs/src/main.rs:425-436` — `OP_RMDIR` arm (currently reuses `unlink_fat16`)
- `cells/services/vfs/src/main.rs:437-446` — `OP_UNLINK` arm (also reuses `unlink_fat16`)
- `cells/services/vfs/src/main.rs:241-243` — `DataFs` / `DataDir` type aliases

## Overview
- **Priority:** P2
- **Status:** pending
- **Description:** fatfs `remove()` deletes either a regular file OR an empty directory.
  Phase G's DRY decision routed both `OP_RMDIR` and `OP_UNLINK` through `unlink_fat16`, so
  `rmdir /data/file.txt` and `unlink /data/emptydir` both wrongly succeed. Add type guards so
  `unlink` accepts only files and `rmdir` accepts only directories.

## Key insights
- fatfs distinguishes entry kind by which open succeeds: `root_dir().open_file(rel)` succeeds
  only for regular files; `root_dir().open_dir(rel)` succeeds only for directories. Probe with
  the matching opener before calling `remove()`.
- `remove()` on a non-empty dir already returns `Err` (POSIX-correct), so non-empty handling
  is unchanged — this phase only fixes the **type** mismatch, not the emptiness check.
- No opcode change. No shell change. No wire-format change. Pure server-side correctness fix.
- `/tmp/` (RamFS) `vfs.rmdir`/`vfs.unlink` already enforce type correctly — leave untouched.

## Requirements
**Functional**
- `unlink_fat16("/data/dir")` → `false` (was: wrongly `true`).
- `unlink_fat16("/data/file.txt")` → `true` (unchanged).
- `rmdir_fat16("/data/file.txt")` → `false` (was: wrongly `true`).
- `rmdir_fat16("/data/emptydir")` → `true` (unchanged).
- `rmdir_fat16("/data/nonemptydir")` → `false` (unchanged — `remove` errors).

**Non-functional**
- DRY: probe-then-remove is the only added logic; no duplicated path-strip code beyond the
  existing per-helper pattern already used by `write_fat16`/`read_fat16`.

## Architecture / data flow
```
OP_UNLINK ("/data/x") ──► unlink_fat16
                           ├─ strip "/data/" → rel
                           ├─ open_file(rel)? ─no─► return false  (it's a dir / missing)
                           └─ yes ─► remove(rel).is_ok()

OP_RMDIR  ("/data/x") ──► rmdir_fat16
                           ├─ strip "/data/" → rel
                           ├─ open_dir(rel)? ─no─► return false   (it's a file / missing)
                           └─ yes ─► remove(rel).is_ok()          (Err if non-empty → false)
```

## Related code files
**Modify**
- `cells/services/vfs/src/main.rs` — rewrite `unlink_fat16`; add `rmdir_fat16`; repoint `OP_RMDIR` arm.

**Create / Delete:** none.

## Implementation steps

### 2a. Rewrite `unlink_fat16` (main.rs:312-322) with a file-only guard
```rust
/// Remove `/data/[sub/]NAME` where NAME is a regular FILE. Returns false if the
/// entry is a directory or does not exist (use OP_RMDIR for directories).
/// Phase H: strict POSIX type checking — `open_file` succeeds only for files in fatfs.
fn unlink_fat16(fs: Option<&DataFs>, path: &str) -> bool {
    let fs  = match fs { Some(f) => f, None => return false };
    let rel = match path.strip_prefix("/data/") {
        Some(n) if !n.is_empty() => n,
        _ => return false,
    };
    if fs.root_dir().open_file(rel).is_err() { return false; }
    fs.root_dir().remove(rel).is_ok()
}
```

### 2b. Add `rmdir_fat16` (place directly after `unlink_fat16`)
```rust
/// Remove an EMPTY `/data/[sub/]DIR`. Returns false if the entry is a regular file,
/// is non-empty, or does not exist. Phase H: strict POSIX type checking.
/// `open_dir` succeeds only for directories; `remove` errors on a non-empty dir.
fn rmdir_fat16(fs: Option<&DataFs>, path: &str) -> bool {
    let fs  = match fs { Some(f) => f, None => return false };
    let rel = match path.strip_prefix("/data/") {
        Some(n) if !n.is_empty() => n,
        _ => return false,
    };
    if fs.root_dir().open_dir(rel).is_err() { return false; }
    fs.root_dir().remove(rel).is_ok()
}
```

### 2c. Repoint the `OP_RMDIR` arm (main.rs:425-436)
Change the `/data/` branch from `unlink_fat16(...)` to `rmdir_fat16(...)`:
```rust
                    OP_RMDIR => {
                        if let Some(p) = path {
                            let ok = if p.starts_with("/data/") {
                                rmdir_fat16(fat_fs.as_ref(), p)
                            } else {
                                vfs.rmdir(p)
                            };
                            ostd::syscall::sys_send(sender, if ok { b"\x00" } else { b"\x01" });
                        }
                    }
```
Update the inline comment at main.rs:427-428 (the old "fatfs remove() deletes an empty dir"
note) to describe the new type-guard behavior.
The `OP_UNLINK` arm (main.rs:437-446) already calls `unlink_fat16` — no change needed; it now
inherits the file-only guard automatically.

### 2d. Compile
```
cargo check -p service-vfs
```

## Todo
- [ ] 2a Rewrite `unlink_fat16` with `open_file` guard
- [ ] 2b Add `rmdir_fat16` with `open_dir` guard
- [ ] 2c Repoint `OP_RMDIR` arm to `rmdir_fat16` + fix comment
- [ ] 2d `cargo check -p service-vfs` clean

## Success criteria (test matrix)
| Op | Target | Expected reply |
|----|--------|----------------|
| `rm /data/f.txt` (file) | file | `0x00` ok |
| `rm /data/d` (dir) | dir | `0x01` err |
| `rmdir /data/d` (empty dir) | empty dir | `0x00` ok |
| `rmdir /data/f.txt` (file) | file | `0x01` err |
| `rmdir /data/nonempty` | non-empty dir | `0x01` err |

Manual QEMU verification (no new automated test in this phase — covered by Phase 3's
recursive test plus existing Phase G rmdir/unlink tests): `cargo check` clean + boot mounts FAT16.

## Risk assessment
| Risk | L×I | Mitigation |
|------|-----|------------|
| `open_file`/`open_dir` probe leaves a stray open handle | Low×Low | Handle dropped at end of `if` expr (RAII); `remove` reopens by path |
| Probe + remove = double traversal cost | Low×Low | Acceptable; paths are short, disk is QEMU-local |
| Phase 3 recursive path needs different semantics | — | Phase 3 uses its own recursive helper, not `rmdir_fat16` directly (see Phase 3) |

## Security considerations
- Tightens, never loosens, deletion semantics. No new path reaches `remove()` that did not before.
- `/data/` prefix authorization unchanged.

## Next steps / dependencies
- **Blocks Phase 3** — recursive rmdir's base case removes an emptied dir and must agree with
  `rmdir_fat16`'s type model.
- Independent of Phase 1 (different crate) and Phase 4 (different opcode).

## Unresolved questions
- None. (fatfs `open_file`/`open_dir` kind-discrimination verified available in fatfs 0.4.)
