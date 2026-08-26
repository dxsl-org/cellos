# Phase 3: Recursive rmdir (`rm -r /data/dir`)

## Context links
- `cells/services/vfs/src/main.rs:241-243` — `DataFs`/`DataDir` aliases
- `cells/services/vfs/src/main.rs:425-436` — `OP_RMDIR` arm (new arm added after it)
- `cells/services/vfs/src/main.rs:32-39` — opcode block (add `OP_RMDIR_RECURSIVE=9`)
- `cells/apps/shell/src/cmd_fs.rs:16-18` — shell opcode consts
- `cells/apps/shell/src/cmd_fs.rs:239-254` — `cmd_rm` (currently strips `-r` silently)
- `cells/apps/shell/src/cmd_fs.rs:44-59` — `vfs_path_op` IPC helper (pattern to mirror)
- `tests/integration/tests/boot.rs` — `QemuRunner`, `CMD_TIMEOUT=10` pattern

## Overview
- **Priority:** P2
- **Status:** pending
- **Description:** Add `OP_RMDIR_RECURSIVE=9` and shell `rm -r /data/dir` to delete a directory
  tree. fatfs `Dir::iter()` yields `DirEntry` with `.is_dir()` / `.file_name()`; recurse
  depth-first, removing files then the now-empty dir.
- **Depends on Phase 2** (the base case removing an emptied dir matches `rmdir_fat16`'s type model).

## Key insights — borrow-checker design (CRITICAL)
The naive recursive helper that passes a `DataDir` by value while also deriving a `target`
sub-`Dir` from it (the form sketched in the task brief) **will not borrow-check**: `target`
borrows from `dir`, but the recursive call also needs `dir`, and `dir.remove()` after the loop
needs it again — overlapping mutable/immutable use of the same `'a`-bound handle.

**Chosen design — recurse on `&DataFs` with full relative paths, rebuild `root_dir()` per call:**
- Each recursion level opens a fresh `root_dir()` handle (cheap; just a struct over the FS ref).
- Children are addressed by their FULL relative path from root (e.g. `"rr/sub/f.txt"`), built
  with `alloc::format!`. fatfs traverses `/`-separated paths natively (same as `write_fat16`).
- `Dir::iter()` borrow is fully released (entries collected into a `Vec<(String,bool)>`) BEFORE
  any `remove()` — avoids iterator-vs-mutation aliasing.
- No `DataDir` is ever held across a recursive call. Each level's handles drop before recursing.

This trades a little allocation (one `format!` per entry) for a design the borrow checker
accepts on the first try. Acceptable per KISS — directory trees here are shallow and small.

## Requirements
**Functional**
- `rmdir_recursive_fat16(fs, "/data/dir")` deletes `dir` and everything under it; returns `true`.
- A `/data/` path that resolves to a regular file is removed directly (POSIX `rm -r file` works).
- Returns `false` on any underlying fatfs error, or on a missing target.
- Shell `rm -r /data/dir` and `rm -rf /data/dir` invoke it; `rm /data/file` keeps OP_UNLINK.
- Recursive rmdir is **rejected for `/tmp/`** (volatile RamFS) — returns `false` (out of scope).

**Non-functional**
- Depth-first, files-before-dir ordering (so each dir is empty when removed).
- Self-referential entries `"."` and `".."` filtered out.

## Architecture / data flow
```
shell: rm -r /data/rr ─► rm_recursive("/data/rr")
         └─ OP_RMDIR_RECURSIVE | path ─► VFS_ENDPOINT(3)
VFS arm OP_RMDIR_RECURSIVE ─► rmdir_recursive_fat16(fs, "/data/rr")
         ├─ strip "/data/" → "rr"
         └─ remove_tree(fs, "rr"):
              ├─ open_dir("rr")? ─no─► remove("rr") (file) ─► is_ok()
              └─ yes ─► iter() → collect [(name,is_dir)]      (borrow released)
                         for each child at "rr/<name>":
                            is_dir → recurse remove_tree(fs, "rr/<name>")
                            file   → fs.root_dir().remove("rr/<name>")
                         finally → fs.root_dir().remove("rr")  (now empty)
```

## Related code files
**Modify**
- `cells/services/vfs/src/main.rs` — add opcode const, helper(s), and `OP_RMDIR_RECURSIVE` arm.
- `cells/apps/shell/src/cmd_fs.rs` — add opcode const, `rm_recursive`, rewrite `cmd_rm`.
- `tests/integration/tests/boot.rs` — add `vfs_fat16_recursive_rmdir` test.

**Create / Delete:** none.

## Implementation steps

### 3a. Add opcode const in `main.rs` (after OP_READ line, main.rs:39)
```rust
const OP_RMDIR_RECURSIVE: u8 = 9; // path -> 0=ok, 1=err — recursive tree delete (/data only)
```

### 3b. Add the recursive helper in `main.rs` (after `rmdir_fat16` from Phase 2)
```rust
/// Recursively remove `/data/[sub/]DIR` and all its contents (POSIX `rm -r`).
/// A path resolving to a regular file is removed directly. Returns false on any
/// fatfs error or missing target. Only `/data/` is supported (caller enforces).
fn rmdir_recursive_fat16(fs: Option<&DataFs>, path: &str) -> bool {
    let fs  = match fs { Some(f) => f, None => return false };
    let rel = match path.strip_prefix("/data/") {
        Some(n) if !n.is_empty() => n,
        _ => return false,
    };
    remove_tree(fs, rel)
}

/// Depth-first delete of `rel` (a path relative to the FAT16 root). Files are
/// removed directly; directories are emptied recursively then removed.
/// Rebuilds `root_dir()` per level and addresses children by full relative path,
/// so no `Dir` handle is held across a recursive call (borrow-checker safe).
fn remove_tree(fs: &DataFs, rel: &str) -> bool {
    // If `rel` is not a directory, treat it as a file (or already-gone) and remove it.
    let dir = match fs.root_dir().open_dir(rel) {
        Ok(d)  => d,
        Err(_) => return fs.root_dir().remove(rel).is_ok(),
    };
    // Collect (name, is_dir) so the iterator borrow is released before we mutate.
    let entries: alloc::vec::Vec<(alloc::string::String, bool)> = dir
        .iter()
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name();
            if name == "." || name == ".." { None } else { Some((name, e.is_dir())) }
        })
        .collect();
    drop(dir); // explicit: ensure no live handle into `fs` across the loop below

    for (name, is_dir) in &entries {
        let child = alloc::format!("{}/{}", rel, name);
        let ok = if *is_dir {
            remove_tree(fs, &child)
        } else {
            fs.root_dir().remove(&child).is_ok()
        };
        if !ok { return false; }
    }
    fs.root_dir().remove(rel).is_ok()
}
```
> NOTE: requires `alloc::vec::Vec`, `alloc::string::String`, `alloc::format!` — the vfs crate
> already links `alloc` (used by `VfsManager`). If `Dir::iter()` needs a trait import
> (`fatfs::Read`/iterator trait), add it as a local `use` inside the fn; verify at compile.
> `e.file_name()` returns `String` in fatfs 0.4 (LossyOemCpConverter) — `==` against `&str` works.

### 3c. Add the `OP_RMDIR_RECURSIVE` arm in `main.rs` (after the `OP_RMDIR` arm)
```rust
                    OP_RMDIR_RECURSIVE => {
                        if let Some(p) = path {
                            // Recursive delete only for the persistent FAT16 volume.
                            // /tmp RamFS is volatile and out of scope (Phase I).
                            let ok = if p.starts_with("/data/") {
                                rmdir_recursive_fat16(fat_fs.as_ref(), p)
                            } else {
                                false
                            };
                            ostd::syscall::sys_send(sender, if ok { b"\x00" } else { b"\x01" });
                        }
                    }
```

### 3d. Add shell opcode const + `rm_recursive` in `cmd_fs.rs`
Add near the existing opcode consts (cmd_fs.rs:16-18):
```rust
const OP_RMDIR_RECURSIVE: u8 = 9;
```
Add `rm_recursive` (mirror the `vfs_path_op` IPC shape, cmd_fs.rs:44-59):
```rust
/// `rm -r` over IPC: send OP_RMDIR_RECURSIVE with a 2-byte header (op, path_len).
pub fn rm_recursive(path: &str) -> bool {
    let pb = path.as_bytes();
    let pl = pb.len().min(253) as u8;
    let mut buf = [0u8; 256];
    buf[0] = OP_RMDIR_RECURSIVE;
    buf[1] = pl;
    buf[2..2 + pl as usize].copy_from_slice(&pb[..pl as usize]);
    syscall::sys_send(VFS_ENDPOINT, &buf[..2 + pl as usize]);
    let mut reply = [0u8; 4];
    match syscall::sys_recv(0, &mut reply) {
        syscall::SyscallResult::Ok(_) => reply[0] == 0,
        _ => false,
    }
}
```

### 3e. Rewrite `cmd_rm` in `cmd_fs.rs:239-254` to detect `-r`
```rust
/// `rm [-r] [-f] <path>` — remove a file, or (with -r on /data) a directory tree.
pub fn cmd_rm<'a>(mut args: core::str::SplitWhitespace<'a>) -> ViResult<()> {
    let mut recursive = false;
    let path = loop {
        match args.next() {
            Some(a) if a.starts_with('-') => { recursive |= a.contains('r'); }
            Some(a) => break a,
            None => { ostd::io::println("Usage: rm [-r] <path>"); return Ok(()); }
        }
    };
    let ok = if recursive && path.starts_with("/data/") {
        rm_recursive(path)
    } else {
        vfs_path_op(OP_UNLINK, path)
    };
    if !ok {
        ostd::io::print("rm: cannot remove '");
        ostd::io::print(path);
        ostd::io::println("'");
    }
    Ok(())
}
```
> Behavior note: `rm -r` on a `/tmp/` path falls through to OP_UNLINK (file-only) — recursive
> /tmp is out of scope, so this is the documented degenerate behavior, not a silent bug.

### 3f. Compile both crates
```
cargo check -p service-vfs
cargo check -p shell
```

### 3g. Add integration test in `tests/integration/tests/boot.rs`
```rust
/// Phase H: recursive directory removal over IPC (rm -r /data/dir).
#[test]
fn vfs_fat16_recursive_rmdir() {
    if !prerequisites_ok() { return; }
    let mut qemu = QemuRunner::boot(&kernel_path(), &disk_path());
    qemu.wait_for("ViCell >", BOOT_TIMEOUT)
        .unwrap_or_else(|e| panic!("no prompt: {e}\n{}", qemu.dump()));
    assert!(qemu.output_contains("FAT16 /data volume mounted"),
        "FAT16 not mounted\n{}", qemu.dump());
    std::thread::sleep(Duration::from_millis(500));

    qemu.send_line("mkdir /data/rr");
    qemu.wait_for("ViCell >", CMD_TIMEOUT).unwrap();
    qemu.send_line("echo X > /data/rr/f.txt");
    qemu.wait_for("ViCell >", CMD_TIMEOUT).unwrap();
    qemu.send_line("rm -r /data/rr");
    qemu.wait_for("ViCell >", CMD_TIMEOUT).unwrap();
    // The child file must now be gone — vcat reports not found.
    qemu.send_line("vcat /data/rr/f.txt");
    qemu.wait_for("not found", CMD_TIMEOUT)
        .unwrap_or_else(|e| panic!("tree not deleted: {e}\n{}", qemu.dump()));
}
```
> Match exact helper names/signatures in the existing `boot.rs` (`prerequisites_ok`, `kernel_path`,
> `disk_path`, `BOOT_TIMEOUT`, `CMD_TIMEOUT`, `output_contains`, `dump`). Verify `echo X > path`
> and `vcat`'s "not found" string against the current shell before finalizing the test asserts.

## Todo
- [ ] 3a Add `OP_RMDIR_RECURSIVE` const in main.rs
- [ ] 3b Add `rmdir_recursive_fat16` + `remove_tree`
- [ ] 3c Add `OP_RMDIR_RECURSIVE` arm
- [ ] 3d Add shell const + `rm_recursive`
- [ ] 3e Rewrite `cmd_rm` with `-r` detection
- [ ] 3f `cargo check -p service-vfs` and `-p shell` clean
- [ ] 3g Add + pass `vfs_fat16_recursive_rmdir` integration test

## Success criteria
- Both crates compile.
- `mkdir /data/rr; echo X > /data/rr/f.txt; rm -r /data/rr` then `vcat /data/rr/f.txt` → "not found".
- `rm /data/file.txt` (no `-r`) still works via OP_UNLINK.
- Nested case (manual): `mkdir /data/a; mkdir /data/a/b; echo X > /data/a/b/c.txt; rm -r /data/a`
  succeeds and the whole tree is gone.

## Risk assessment
| Risk | L×I | Mitigation |
|------|-----|------------|
| Borrow-checker aliasing on recursive `Dir` | High×High | `&DataFs` recursion + per-level `root_dir()` + full-path children; collect entries before mutate (design above) |
| `iter()` yields `.`/`..` causing infinite recursion | Med×High | Explicitly filter `"."`/`".."` |
| `e.file_name()` type mismatch vs `&str` compare | Low×Med | fatfs returns `String`; verify at compile; adjust to `.as_str()` if needed |
| Deep tree → stack overflow (kernel/cell stack) | Low×Med | Trees are shallow in practice; document; iterative rewrite deferred if a test ever overflows |
| `format!` allocation fails (OOM) | Low×Low | `remove` of a partial tree already returned false on first error; acceptable best-effort |

## Security considerations
- Recursive delete is gated to `/data/` only; `/tmp/` and all other prefixes return `false`.
- Same `/data/` authorization as every other write op — no new privilege.
- Worst case is over-deletion within `/data/` (user-intended for `rm -r`); no path escapes `/data/`
  because every child path is built as `format!("{rel}/{name}")` under the stripped `rel`.

## Next steps / dependencies
- Requires Phase 2 merged first (shared file `main.rs`; base case agrees with `rmdir_fat16`).
- Serialize before Phase 4 (both edit `main.rs`, `cmd_fs.rs`, `boot.rs`).

## Unresolved questions
- Does `fatfs::Dir::iter()` require importing a specific trait in this crate's edition? Resolve at
  compile (3f) — add the local `use` if the method is not inherent.
- Confirm shell supports `echo X > /data/...` redirection; if not, the test should `write_file`
  via an existing built-in (e.g. a `vwrite`/`touch`+write path) — verify against current shell.
