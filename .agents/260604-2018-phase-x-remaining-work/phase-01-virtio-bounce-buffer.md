# Phase 01 — VirtIO Bounce Buffer (X-1)

**Priority:** P0 | **Effort:** ~1h | **Status:** pending | **Files:** 2

## Context Links
- Crash root cause: `VirtioHal::share()` returns `vaddr as usize` as the PA,
  assuming identity mapping. Stack pages are identity-mapped; ELF BSS pages are
  NOT — DMA lands at the wrong physical address for BSS buffers.
- Ignored test: `tests/integration/tests/boot.rs:734` (`#[ignore]` + comment at 731-733)

## Overview
Restore the `vfs_fat16_recursive_rmdir` test. The `BlkRead`/`BlkWrite` syscall
handlers pass the user buffer straight to the VirtIO driver, which treats its
virtual address as physical. A kernel-stack bounce buffer is always identity-
mapped, so DMA targets a correct PA; the kernel then copies to/from the user
buffer (SUM=1 already permits S-mode access to U-mode pages).

## Key Insights
- fatfs's stack `[0u8; 512]` works only by accident (stack is identity-mapped).
- The fix is local to two handlers — no driver or ABI change (Law 1 untouched).
- 512-byte double-copy per sector is the cost; acceptable for single-threaded VFS.

## Architecture / Data Flow
**BlkRead (new):** device → `bounce[512]` (kernel stack, identity-mapped, valid
DMA target) → `copy_from_slice` → user buf (SUM=1).
**BlkWrite (new):** user buf (SUM=1) → `copy_from_slice` → `bounce[512]` →
device.

## Related Code Files
- Modify: `kernel/src/task/syscall.rs` — `BlkRead` (1149-1169), `BlkWrite` (1170-1190)
- Modify: `tests/integration/tests/boot.rs` — remove `#[ignore]` at line 734, update comment 731-733

## Implementation Steps
1. In `BlkRead` (syscall.rs:1164-1168): replace the direct `from_raw_parts_mut`
   pass-through. Read into `let mut bounce = [0u8; 512];`, then on `Ok(())`
   build the user slice and `buf.copy_from_slice(&bounce);` return `Ok(1)`.
   Keep `validate_user_buf` and the `CELL_TABLE_BASE_LBA` guard unchanged.
2. In `BlkWrite` (syscall.rs:1185-1189): build user slice via
   `from_raw_parts(buf_ptr, 512)`, `let mut bounce = [0u8; 512];`,
   `bounce.copy_from_slice(user)`, then `write_sector(sector, &bounce)`.
3. Keep the existing `// SAFETY:` comments; extend them to note the bounce
   buffer rationale (identity-mapped kernel stack = valid DMA PA).
4. Remove `#[ignore]` at boot.rs:734; rewrite the 731-733 comment to record the
   bounce-buffer fix instead of the "pending SAS fix" note.
5. Rebuild: `cargo build --release -p ViCell-kernel` → `./gen_disk.ps1` → run test.

## Todo List
- [ ] BlkRead bounce buffer
- [ ] BlkWrite bounce buffer
- [ ] Extend SAFETY comments
- [ ] Un-ignore test + update comment
- [ ] Rebuild kernel + disk, run test green

## Success Criteria
- `cargo test -p integration-tests vfs_fat16_recursive_rmdir` passes with NO
  `#[ignore]` attribute (observable: 56 tests pass, 0 ignored).
- All previously-passing tests still pass (no regression in block I/O paths:
  `cat`, VFS read/write tests).

## Risk Assessment
| Risk | L×I | Mitigation |
|------|-----|------------|
| Double-copy perf regression | Low×Low | 512 B/sector, single-threaded VFS — negligible |
| Stack pressure from `[0u8;512]` in handler | Low×Med | Kernel syscall stack is large; one frame, not nested |
| Other callers of read_sector rely on old path | Low×Med | Driver unchanged; only syscall handlers edited — grep confirms handlers are the only user-buffer entry |

## Rollback
Revert the two handler edits and re-add `#[ignore]`. No data/ABI migration —
the change is purely an internal copy step.

## Security Considerations
`validate_user_buf` + `CELL_TABLE_BASE_LBA` guard remain the security boundary;
unchanged. Bounce buffer never exposes kernel memory (zeroed local, fully
overwritten by device read before copy-out).

## Next Steps
None — self-contained. Unblocks full block-I/O test coverage for later phases.
