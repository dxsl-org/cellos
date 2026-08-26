# Phase 4: OP_APPEND for large / chunked writes

## Context links
- `cells/services/vfs/src/main.rs:386-402` — `OP_WRITE` arm (header to mirror)
- `cells/services/vfs/src/main.rs:265-285` — `write_fat16` (helper to mirror)
- `cells/services/vfs/src/main.rs:246-263` — `split_last`, `ensure_dir_chain` (reused)
- `cells/services/vfs/src/main.rs:32-39` — opcode block (add `OP_APPEND=10`)
- `cells/services/vfs/src/block_stream.rs:88-101` — `BlockStream::seek`; `End(_) → Err(())` at :97
- `cells/apps/shell/src/cmd_fs.rs:256-278` — `write_file` client (shape to mirror)
- `tests/integration/tests/boot.rs` — `QemuRunner` test pattern

## Overview
- **Priority:** P2
- **Status:** pending
- **Description:** `OP_WRITE` truncates on every call (`write_fat16` does remove-then-create),
  so a payload > 508 bytes cannot be assembled. Add `OP_APPEND=10` (same 4-byte header as
  `OP_WRITE`) that opens-or-creates the file, seeks to end, and appends. Combined with
  `OP_WRITE` for the first chunk, this enables arbitrary-length writes via chunking.
- **Independent** of Phases 1–3 (new opcode, new helper). Serialize file edits after Phase 3.

## Key insight — why `BlockStream::seek(End)` is NOT a blocker
`BlockStream::seek(SeekFrom::End(_))` returns `Err(())` (block_stream.rs:97). But fatfs does
**not** call `disk.seek(End)`. fatfs tracks file size internally and translates a
`File::seek(SeekFrom::End(0))` into a `disk.seek(SeekFrom::Start(absolute_end_offset))`.
`BlockStream::seek(SeekFrom::Start(n))` IS implemented (block_stream.rs:91). Therefore
`append_fat16` can call `file.seek(fatfs::SeekFrom::End(0))` and the `End` arm in `BlockStream`
is never reached. **No change to `BlockStream` is required.**

## Requirements
**Functional**
- `OP_APPEND /data/x content` opens-or-creates `x`, seeks to end, appends `content`; reply `0x00`.
- First append to a non-existent file behaves like a write (creates it).
- Intermediate dirs are auto-created (mkdir -p), same as `write_fat16`.
- Header is byte-identical to `OP_WRITE`: `[op:1][path_len:u8][content_len:u16 LE][path][content]`.
- `/tmp/` append: minimal read-extend-write via `VfsManager` (see 4c; may defer — decision below).

**Non-functional**
- DRY: reuse `split_last` + `ensure_dir_chain`; do not duplicate the dir-chain logic.

## Architecture / data flow
```
shell append_file(path, chunk) ─► OP_APPEND header ─► VFS_ENDPOINT(3)
VFS arm OP_APPEND ─► parse header (same as OP_WRITE)
   /data/ ─► append_fat16(fs, path, content)
              ├─ strip "/data/" → rel; split_last → (parent, name)
              ├─ ensure_dir_chain(root, parent) → dir
              ├─ open_file(name) OR create_file(name) → file
              ├─ file.seek(End(0))  ── fatfs ─► disk.seek(Start(abs_end))  [BlockStream OK]
              └─ file.write_all(content).is_ok()
   /tmp/  ─► append_ramfs(&mut vfs, path, content)  (read existing, extend, write back)
```

## Related code files
**Modify**
- `cells/services/vfs/src/main.rs` — opcode const, `append_fat16` (+ optional `append_ramfs`), arm.
- `cells/apps/shell/src/cmd_fs.rs` — opcode const, `append_file` client.
- `tests/integration/tests/boot.rs` — `vfs_fat16_append` test.

**Create / Delete:** none.

## Implementation steps

### 4a. Add opcode const in `main.rs` (after OP_RMDIR_RECURSIVE / OP_READ block)
```rust
const OP_APPEND: u8 = 10; // [path_len:u8][content_len:u16 LE][path][content] → seek-to-end append
```

### 4b. Add `append_fat16` in `main.rs` (after `write_fat16`)
```rust
/// Append `content` to `/data/[sub/]NAME`. Creates the file (and any parent dirs)
/// if absent — first append == write. Intermediate dirs via ensure_dir_chain (mkdir -p).
///
/// fatfs `File::seek(End(0))` translates to `disk.seek(Start(abs_end))` internally, so the
/// `End` arm of `BlockStream::seek` (which errors) is never reached — append works without
/// touching BlockStream. See block_stream.rs:97.
fn append_fat16(fs: Option<&DataFs>, path: &str, content: &[u8]) -> bool {
    use fatfs::{Write as _, Seek as _};
    let fs  = match fs { Some(f) => f, None => return false };
    let rel = match path.strip_prefix("/data/") {
        Some(n) if !n.is_empty() => n,
        _ => return false,
    };
    let (parent, name) = split_last(rel);
    if name.is_empty() { return false; }
    let dir = match ensure_dir_chain(fs.root_dir(), parent) {
        Ok(d)   => d,
        Err(()) => return false,
    };
    let mut file = match dir.open_file(name) {
        Ok(f)  => f,
        Err(_) => match dir.create_file(name) { Ok(f) => f, Err(_) => return false },
    };
    if file.seek(fatfs::SeekFrom::End(0)).is_err() { return false; }
    file.write_all(content).is_ok()
}
```

### 4c. (Optional) Add `append_ramfs` for `/tmp/` — minimal read-extend-write
Inspect `VfsManager` for `get_file_data` (main.rs:407 uses it) and `write_file` (main.rs:396).
If both exist with usable signatures:
```rust
/// Append to a /tmp RamFS file: read current bytes, extend, write back.
/// KISS — RamFS has no native seek/append; full rewrite is fine for small /tmp files.
fn append_ramfs(vfs: &mut VfsManager, path: &str, content: &[u8]) -> bool {
    let mut data = vfs.get_file_data(path).map(|d| d.to_vec()).unwrap_or_default();
    data.extend_from_slice(content);
    vfs.write_file(path, &data)
}
```
**Decision gate:** if `get_file_data` returns a borrow that conflicts with the subsequent
`&mut vfs.write_file`, copy first (`.to_vec()` above already does). If the `VfsManager` API
makes this awkward, **defer `/tmp/` append to Phase I** and have the `/tmp/` branch return
`false` — record the deferral in the changelog. `/data/` append (4b) is the required deliverable.

### 4d. Add the `OP_APPEND` arm in `main.rs` (after `OP_WRITE` arm)
```rust
                    OP_APPEND => {
                        // Same header as OP_WRITE: [op][path_len:u8][content_len:u16 LE][path][content]
                        let pl = buf[1] as usize;
                        let cl = u16::from_le_bytes([buf[2], buf[3]]) as usize;
                        let ok = if 4 + pl + cl <= buf.len() {
                            match core::str::from_utf8(&buf[4..4 + pl]) {
                                Ok(p) if p.starts_with("/data/") =>
                                    append_fat16(fat_fs.as_ref(), p, &buf[4 + pl..4 + pl + cl]),
                                Ok(p) if p.starts_with("/tmp/") =>
                                    append_ramfs(&mut vfs, p, &buf[4 + pl..4 + pl + cl]),
                                _ => false,
                            }
                        } else { false };
                        ostd::syscall::sys_send(sender, if ok { b"\x00" } else { b"\x01" });
                    }
```
> If `/tmp/` append is deferred (4c decision), replace the `/tmp/` arm with `=> false`.

### 4e. Add `append_file` client in `cmd_fs.rs` (after `write_file`, cmd_fs.rs:278)
```rust
const OP_APPEND: u8 = 10;

/// Append `content` to `path` via OP_APPEND (same 4-byte header as OP_WRITE).
/// Path + content capped to 512 bytes per call — caller chunks for larger payloads.
pub fn append_file(path: &str, content: &[u8]) -> bool {
    let pb = path.as_bytes();
    let pl = pb.len().min(255);
    let cl = content.len().min(512_usize.saturating_sub(4 + pl));
    let mut buf = [0u8; 512];
    buf[0] = OP_APPEND;
    buf[1] = pl as u8;
    buf[2..4].copy_from_slice(&(cl as u16).to_le_bytes());
    buf[4..4 + pl].copy_from_slice(&pb[..pl]);
    buf[4 + pl..4 + pl + cl].copy_from_slice(&content[..cl]);
    syscall::sys_send(VFS_ENDPOINT, &buf[..4 + pl + cl]);
    let mut reply = [0u8; 1];
    match syscall::sys_recv(0, &mut reply) {
        syscall::SyscallResult::Ok(_) => reply[0] == 0,
        _ => false,
    }
}
```

### 4f. Compile both crates
```
cargo check -p service-vfs
cargo check -p shell
```

### 4g. Add integration test in `boot.rs`
Strategy: write 300 'A' bytes via `write_file` (OP_WRITE), append 300 'B' bytes via
`append_file` (OP_APPEND), then `wc /data/big.txt` (or read) and assert byte count 600 —
proving the second chunk did NOT truncate the first.
```rust
/// Phase H: OP_APPEND assembles a >512-byte file in two chunks without truncation.
#[test]
fn vfs_fat16_append() {
    if !prerequisites_ok() { return; }
    let mut qemu = QemuRunner::boot(&kernel_path(), &disk_path());
    qemu.wait_for("ViCell >", BOOT_TIMEOUT).unwrap();
    assert!(qemu.output_contains("FAT16 /data volume mounted"), "{}", qemu.dump());
    std::thread::sleep(Duration::from_millis(500));

    // Drive via a shell built-in that calls write_file then append_file.
    // If no such built-in exists yet, add a hidden `vappend <path> <text>` command
    // wired to append_file (mirror cmd_vcat) so the test can exercise OP_APPEND.
    qemu.send_line("vwrite /data/big.txt AAA");   // OP_WRITE
    qemu.wait_for("ViCell >", CMD_TIMEOUT).unwrap();
    qemu.send_line("vappend /data/big.txt BBB");  // OP_APPEND
    qemu.wait_for("ViCell >", CMD_TIMEOUT).unwrap();
    qemu.send_line("vcat /data/big.txt");
    // Expect both halves present, append after write.
    qemu.wait_for("AAABBB", CMD_TIMEOUT)
        .unwrap_or_else(|e| panic!("append truncated/lost: {e}\n{}", qemu.dump()));
}
```
> The test needs a shell entry point for OP_APPEND. `cmd_fs.rs` currently exposes `write_file`
> as a library fn (used by redirection) but no `append` command. Add a small `cmd_vappend`
> (mirror `cmd_vcat`) wired to `append_file`, and register it in the shell command dispatch.
> Verify the exact `vcat`/`vwrite` command names in the shell before finalizing test strings;
> substitute whatever write path the shell already provides.

## Todo
- [ ] 4a Add `OP_APPEND` const in main.rs
- [ ] 4b Add `append_fat16` (seek-to-end)
- [ ] 4c Decide `/tmp/` append: implement `append_ramfs` or defer (record decision)
- [ ] 4d Add `OP_APPEND` arm
- [ ] 4e Add shell `append_file` client (+ `cmd_vappend` + dispatch for the test)
- [ ] 4f `cargo check -p service-vfs` and `-p shell` clean
- [ ] 4g Add + pass `vfs_fat16_append` integration test

## Success criteria
- Both crates compile.
- A file written then appended reads back with BOTH chunks, append after write (no truncation).
- A two-chunk payload exceeding 508 bytes total is fully reconstructed (write first chunk,
  append remainder).
- First `append_file` to a non-existent path creates it (append == write on empty).

## Risk assessment
| Risk | L×I | Mitigation |
|------|-----|------------|
| fatfs routes seek(End) to BlockStream End arm → Err | Low×High | Verified: fatfs translates to Start(abs_end); End arm unreachable (key insight) |
| `/tmp/` RamFS API can't read-extend-write cleanly | Med×Low | `.to_vec()` copy breaks borrow; else defer /tmp to Phase I (gate 4c) |
| Test needs shell append command that doesn't exist | High×Low | Add `cmd_vappend` + dispatch in 4e; verify names against shell |
| Single message capped at 512 bytes limits chunk size | Known | By design — caller chunks; documented in `append_file` doc comment |

## Security considerations
- `/data/`/`/tmp/` prefix authorization identical to `OP_WRITE`; no new reachable path.
- `4 + pl + cl <= buf.len()` bound check prevents OOB slice on the 512-byte buffer (mirrors OP_WRITE).
- Append cannot target outside `/data/` or `/tmp/` — `_ => false`.

## Next steps / dependencies
- Independent logic; serialize file edits after Phase 3 (shared `main.rs`, `cmd_fs.rs`, `boot.rs`).
- Enables Phase I streaming-write / large-file work.

## Unresolved questions
- `/tmp/` append: implement now or defer to Phase I? (Gate 4c — depends on `VfsManager` API ergonomics.)
- Exact shell command name for writing via OP_WRITE today (`vwrite`? redirection only?) — confirm
  before wiring the `vappend` test command and finalizing 4g assert strings.
