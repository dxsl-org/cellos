---
phase: 05
title: Route VFS + resolve snapshot off kernel block
tier: thinking
status: pending
depends_on: [02, 04]
---

# Phase 05 — Route remaining consumers off kernel block

## Context links
- Plan: [plan.md](plan.md) · Scout: [scout-report.md](scout-report.md)

## Overview
Sever the last kernel `block::read_sector` consumers so Phase 06 can delete the driver. After Phase 04 two consumer classes remain: (a) the VFS syscall fallback (500/501/503/212) and (b) `snapshot.rs` (warm-boot save/restore) + `verify_mbr`.

## Key insights
- VFS already prefers `service::BLOCK_DRIVER` (`block_stream.rs:42-84`); the kernel `sys_blk_*` syscalls are only the *fallback*. Once the Block Cell (RISC-V/ARM) + NVMe cell (x86, F4) are reliable, the fallback is dead code — but must be removed deliberately, not left dangling (silent-fallback = RC-1/RC-2 class).
- **`snapshot.rs` restore is bootstrap-critical**: reads P3 at `main.rs:424` *before any cell exists*. It cannot use the Block Cell. This is the one genuine remaining pre-cell block reader.
- `verify_mbr` (`disk_layout.rs:110`) reads LBA 0 at boot — pre-cell, warn-only, non-essential.

## Requirements
- **Functional:** VFS block I/O goes exclusively through a Driver Cell (virtio-blk cell on RISC-V/ARM, NVMe cell on x86); kernel `sys_blk_*` handlers removed or demoted to a documented boot-only shim.
- **Decision:** record the snapshot resolution (ADR) — gates Phase 06.

## Architecture — snapshot resolution (choose one, record as ADR)
- **(a) Descope warm-boot snapshot during transition (recommended):** feature-gate `snapshot::try_restore/save`; warm-boot reverts to cold-boot. Snapshot is already debt slated for the **Supervisory Cell**, which will own its block access. Lowest risk; keeps this plan focused. Log cold-boot explicitly.
- **(b) Minimal raw snapshot reader:** keep a ~50-line RO sector reader purely for snapshot restore — preserves warm-boot but re-introduces a tiny device dependency (contradicts "zero device driver"). Only if warm-boot is a hard requirement now.

Recommendation: **(a)** — makes "zero kernel device driver" literally true; defers snapshot correctly.

## Related code files
- Modify: `cells/services/vfs/src/block_stream.rs` (require BLOCK_DRIVER; fail-loud on absence — spec 17 §7 — instead of silently using the kernel driver being deleted), `kernel/src/task/syscall.rs` (demote/remove BlkRead/Write/Flush/ReadAsync), `kernel/src/snapshot.rs` (feature-gate per (a)), `kernel/src/main.rs` (guard `try_restore`/`verify_mbr`).
- Write: `docs/adr/` entry for the snapshot decision.

## Implementation steps
1. Record the snapshot ADR (recommend (a)).
2. VFS: require `service::BLOCK_DRIVER`; on absence fail-loud, no kernel fallback.
3. Feature-gate snapshot save/restore (a) or add the minimal reader (b).
4. Guard/remove `verify_mbr` at boot.
5. Demote kernel `sys_blk_*` handlers: `NotSupported` (fail-loud) or delete, per remaining callers.
6. Boot 3-arch; temporary debug counter proves **zero** kernel `block::read_sector` calls at runtime, then remove the counter.

## Todo
- [ ] Snapshot ADR recorded
- [ ] VFS requires BLOCK_DRIVER + fail-loud
- [ ] snapshot save/restore feature-gated (or minimal reader)
- [ ] verify_mbr guarded/removed
- [ ] kernel `sys_blk_*` demoted/removed
- [ ] zero-kernel-block-call proof at runtime

## Success criteria
- **Runtime evidence:** boot log + temporary counter show zero kernel `block::read_sector`/`write_sector` calls after boot; VFS read/write correct via the Driver Cell; if warm-boot descoped, log states cold-boot. Suite green 3-arch.

## Risk assessment
- *Silent fallback masking a broken Block Cell* — the fail-loud VFS change (step 2) is essential (repeat of RC-1/RC-2 silent-degrade class).
- *Warm-boot users surprised* — changelog note; returns with the Supervisory Cell plan.

## Security considerations
- Removing kernel `sys_blk_*` shrinks the kernel's DMA-capable surface. Partition-access gate moves with requests to the Driver Cell boundary (kernel still enforces which cell touches which partition via caps).

## Next steps
Phase 06 deletes the now-unused kernel driver.
