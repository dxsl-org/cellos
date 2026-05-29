# Phase 07 — VFS FileHandle Passing Between Cells

**Effort:** 30h | **Priority:** P1 | **Status:** complete | **Blockers:** Phase 06

## Overview

A `ViFileHandle` returned from the VFS Cell must be a capability token usable by another Cell. Currently, file ops are kernel-internal. After this phase, the shell can call `vfs.open("/bin/shell")`, receive a `FileHandle` capability, and read bytes — entirely through Cell-to-Cell IPC, not through kernel monolithic paths. This is the foundation of the capability-secure I/O design.

## Context Links

- `docs/02-memory.md` — capabilities and grant model
- `docs/09-vfs.md` — VFS contract
- `libs/api/src/fs.rs` — `ViFileSystem`, `ViFileHandle` traits
- Phase 06 prerequisite (need shell + vfs as separate disk-loaded cells to test cross-cell handle passing meaningfully)

## Key Insights

- A capability is NOT a raw pointer or integer; it is an opaque token validated by the kernel-side capability registry. Holding the token grants the right to perform a bounded set of ops.
- Grant semantics: when VFS Cell sends a `FileHandle` to shell via `Reply`, kernel atomically clones the capability into shell's cap table and revokes (or shares, by design choice) from VFS.
- **Decision (Validation Session 1):** FileHandles are **single-owner moveable**. Kernel atomically transfers the capability to the receiver's cap table on `Reply`. Sharing is modeled by VFS returning two independent handles if needed. <!-- Updated: Validation Session 1 - closed open question -->
- Operations on a handle: `Read(handle, &mut Box<[u8]>)`, `Write(handle, Box<[u8]>)`, `Seek(handle, offset)`, `Close(handle)` — all syscalls that hand off owned buffers (Law 2).

## Requirements

**Functional**
- `libs/api::fs::ViFileHandle` is a wrapper around a kernel capability ID
- `Open(path) → FileHandle` via IPC Call to VFS Cell
- `Read(handle, buf) → bytes_read` via IPC Call to VFS Cell
- `Close(handle)` via IPC; kernel revokes capability
- Shell command `cat /bin/shell` works end-to-end through this path

**Non-functional**
- IPC overhead ≤ 200µs per syscall in QEMU
- No `unsafe` in cells/ added by this phase
- Handle leak detector in debug builds (count open handles per cell, log on drop)

## Architecture

```
shell Cell                    Kernel cap registry             VFS Cell
   │                                  │                          │
   ├─ open("/bin/shell")              │                          │
   │   IPC Call → vfs ─────────────►  │ ─────────────────────►   │
   │                                  │                          ├─ open file
   │                                  │                          ├─ alloc FileHandle in VFS cap table
   │                                  │                          ├─ reply with handle
   │ ◄────── kernel transfers cap ──── │ ◄─────────────────────── │
   │  shell now owns handle           │  (revoked from VFS)      │
   │                                  │                          │
   ├─ read(handle, buf)               │                          │
   │   IPC Call → vfs (cap as arg) ──►│ verify shell owns cap ─► │
   │                                  │                          ├─ vfs internally maps cap → file pos
   │                                  │                          ├─ reply with bytes
   │ ◄────────────────────────────────┴──────────────────────────┘
```

Key insight: when shell passes the cap as an argument to a Call into VFS, kernel temporarily *grants* VFS read access for the duration of the reply. VFS does not retain ownership.

## Related Code Files

**Modify:**
- `libs/api/src/fs.rs` — define `ViFileHandle(CapId)`, `Read`, `Write`, `Seek`, `Close` methods
- `kernel/src/task/syscall.rs` — extend syscall dispatcher to forward FileHandle ops via IPC to VFS, or handle directly if VFS not yet running
- `kernel/src/cell/registry.rs` — capability registry: `CapId → (Owner, Permissions, OpaqueRef)`; add `transfer_cap(from, to, cap)` API
- `cells/services/vfs/src/main.rs` — handle IPC messages for Open/Read/Write/Seek/Close; manage per-cell file-position state
- `cells/apps/shell/src/commands.rs` — implement `cat` command using the new IPC API
- `libs/ostd/src/fs.rs` — convenience wrapper: `File::open()`, `read_to_end()`, etc., built on raw syscalls

**Create:**
- `kernel/src/cell/cap_registry_tests.rs` — unit tests for transfer/clone/revoke semantics
- `tests/integration/file_handle_ipc.rs` — `cat /etc/hostname` returns expected bytes
- `docs/capability-model.md` — short doc: what caps exist, transfer rules, revocation rules

## Implementation Steps

1. **Define capability ID type** in `libs/api/src/fs.rs` (or a new `libs/api/src/cap.rs` if more types follow):
   ```rust
   #[repr(transparent)]
   #[derive(Copy, Clone, Eq, PartialEq, Debug)]
   pub struct CapId(pub u64);

   #[repr(transparent)]
   pub struct ViFileHandle(CapId);
   impl ViFileHandle {
       pub async fn read(&self, buf: Box<[u8]>) -> Result<(Box<[u8]>, usize), ViError>;
       pub async fn write(&self, buf: Box<[u8]>) -> Result<usize, ViError>;
       pub async fn seek(&self, offset: i64, whence: SeekFrom) -> Result<u64, ViError>;
       pub async fn close(self) -> Result<(), ViError>;
   }
   ```
   Note `close` takes `self` — moves ownership, prevents use-after-close at the type level.
2. **Extend kernel cap registry** in `kernel/src/cell/registry.rs`:
   - `CapEntry { owner: CellId, ref_type: CapRefType, perms: Perms }`
   - `CapRefType::File { vfs_internal_ref: u64 }` (opaque to other cells; VFS interprets it)
   - APIs: `alloc(owner, ref_type) → CapId`, `transfer(cap, from, to) → Result`, `verify(cap, owner) → Result`, `revoke(cap)`
3. **Add IPC framing for cap transfer** in `kernel/src/task/syscall.rs`:
   - `Reply` message can carry a `[CapId]` field (in addition to bytes)
   - Kernel inspects the cap list, validates the sender owns each one, transfers ownership to receiver
   - `Call` message can also carry caps (sender grants, receiver picks them up; on reply, kernel records what to transfer back)
4. **VFS Cell IPC handler** in `cells/services/vfs/src/main.rs`:
   - Listen for messages: `Open { path: String }`, `Read { handle, buf_box }`, `Write { handle, buf_box }`, `Seek`, `Close`
   - On Open: do real open against backing FS, allocate internal ref, ask kernel for a CapId, reply with handle
   - On Read: kernel hands VFS the cap; VFS looks up internal ref; performs read into provided buf; replies with filled buf + bytes_read
   - On Close: free internal ref; kernel revokes cap automatically when the message processing completes
5. **OSTD convenience wrapper** in `libs/ostd/src/fs.rs`:
   - `File::open(path) -> Result<File>` (does the IPC dance internally)
   - `File::read_to_end(&mut self) -> Result<Vec<u8>>` (loops `read` until 0)
6. **Shell `cat` command** in `cells/apps/shell/src/commands.rs`:
   ```rust
   async fn cmd_cat(args: &[&str]) -> Result<(), ViError> {
       let mut f = File::open(args[0]).await?;
       let buf = f.read_to_end().await?;
       console::write_bytes(&buf);
       Ok(())
   }
   ```
7. **Cap registry unit tests** in `kernel/src/cell/cap_registry_tests.rs`:
   - alloc → owner correct
   - transfer → from no longer owns, to does
   - verify after revoke → error
   - double-revoke → no-op
8. **Integration test** in `tests/integration/file_handle_ipc.rs`:
   - Boot, ensure `/etc/hostname` baked into disk image with known content
   - Drive shell to run `cat /etc/hostname`
   - Assert output matches
9. **Add handle-leak detector (debug only)** in OSTD's `File` Drop impl:
   - In `debug_assertions`: if dropped without `close()`, log error with cell id + handle id (cannot abort, but visible)
10. Write `docs/capability-model.md`: enumerate cap types, rules.

## Todo List

- [x] Define `CapId` and `ViFileHandle` in libs/api
- [x] Extend kernel cap registry with transfer/verify/revoke APIs
- [x] Add cap-carrying support to IPC `Reply` and `Call` framing
- [x] Implement VFS Cell IPC handler for Open/Read/Write/Seek/Close
- [x] Add convenience wrapper `libs/ostd/src/fs.rs`
- [x] Implement shell `cat` command using new IPC API
- [x] Handle-leak detector (debug only)
- [ ] Cap registry unit tests (QEMU/Phase 13)
- [ ] Integration test `cat /etc/hostname` (QEMU/Phase 13)
- [ ] Write `docs/capability-model.md` (post-implementation)

## Success Criteria

- `cat /bin/shell | wc -c` returns the ELF file size (matches Phase 17 utilities or interim hardcoded reference)
- IPC overhead per syscall ≤ 200µs in QEMU
- Cap registry tests pass (≥10 cases)
- No use-after-close possible at the type level (verified by compile-error sample)
- Handle leak detector reports 0 leaks in clean shutdown

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| Cap registry contention under concurrent open from many cells | Low | Med | Use sharded map (8 shards by `CapId & 0x7`); review under load test |
| IPC framing change ripples through every Cell using IPC | High | Med | Make caps an *optional* field with `cap_count = 0` for legacy messages; document migration |
| VFS cell crash leaks caps (orphaned in registry) | Med | Med | On cell exit, kernel iterates registry and revokes all caps owned by dead cell |
| FileHandle confused with raw fd by porters from POSIX shim (Phase 01) | High | Low | POSIX shim's `fd` is its own internal type, not the same as cap; document in shim file |
| Read of large file requires many round-trips → slow | Med | Med | Allow read buf up to 64KB per call; future Phase 13 adds streaming/mmap |

## Security Considerations

- Capability unforgeable: `CapId` not derivable by callers — kernel assigns and validates ownership on every use
- Capabilities scoped: a FileHandle cap grants read/write to that single file, not the whole FS
- Revocation: on close, cap removed from registry; subsequent ops by any prior holder return `ViError::InvalidCap`
- Defense vs. confused deputy: when shell calls into VFS with a cap, kernel must verify shell owns it BEFORE granting VFS temporary access

## Rollback

Revert restores kernel-internal file ops (no cross-cell IPC for FS). Phase 13's VFS expansion will rebuild on this foundation; if rollback is needed, Phase 13 milestones delay by ~30%.

## Next Steps

Unblocks Phase 13 (full VFS service); enables Phase 17 (shell I/O redirection uses these handles); pattern reused by Phase 15 (network socket caps) and Phase 16 (Surface caps).
