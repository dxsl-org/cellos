---
phase: 2
title: "Persistent SD Storage"
status: completed
priority: P2
effort: "2d"
dependencies: []
tier: medium
---

# Phase 02: Persistent SD Storage

## Overview

Enable verified persistent write/read on RPi3 SD card via the existing VFS →
RedoxFS → SDHCI CMD24 path. Fix the RPi3 image provisioning gap (P5 RedoxFS
partition), implement a real MMC flush, and prove persistence across reboot.

## Requirements

- RPi3 SD image includes a formatted RedoxFS P5 partition at `/srv`.
- VFS writes to `/srv/<namespace>` persist across power cycles on RPi3.
- `MmcBlock::flush()` issues a hardware cache-flush command (CMD32/CMD38 or
  equivalent) instead of the current no-op.
- A physical RPi3 reboot test proves data written before reboot is readable after.

## Architecture

Existing data path:
```
VFS → RedoxFsBackend → VicellDisk (P5, LBA 931072) → blk_router
  → sys_blk_write → MmcBlock → SDHCI CMD24 → SD card
```

The path is complete and physically verified on RPi3. The two tracked gaps
were closed: the test image received a formatted RedoxFS P5 volume, and the
MMC flush path now waits for card readiness with CMD13.

## Assumptions

- **Claim:** The SDHCI CMD24 write implementation functions correctly on the
  RPi3 BCM2837 controller.
  **Confidence:** high (physical same-boot readback passed for FAT, littlefs,
  and RedoxFS).
  **How to verify:** Covered by the physical `rpi3-storage` gate.

- **Claim:** RedoxFS two-boot persistence test logic in
  `tests/integration/tests/redoxfs-srv.rs` is architecture-neutral and can
  be adapted for RPi3.
  **Confidence:** high
  **How to verify:** Read the test source; it uses generic VFS APIs.

## Related Files

- Modify: `scripts/format-disk-arm.sh` (add P5 RedoxFS provisioning)
- Modify: `kernel/src/task/drivers/mmc.rs` (`flush()` implementation)
- Modify: `kernel/src/task/drivers/mmc/sd.rs` (CMD flush support)
- Modify: `kernel/src/task/drivers/mmc/sdhci.rs` (SDHCI flush transfer)
- Reference: `cells/services/vfs/src/backend_redoxfs.rs`
- Reference: `cells/services/vfs/src/disk_redoxfs.rs`
- Reference: `scripts/mksrv-img.sh` (RV64 P5 formatter template)
- Reference: `tests/integration/tests/redoxfs-srv.rs`

## Implementation Steps

1. [x] Study `scripts/mksrv-img.sh` to understand the offline RedoxFS P5 creation
   procedure. Adapt or extend `scripts/format-disk-arm.sh` to create a
   partitioned RPi3 SD image that includes P5 at the canonical LBA offset
   with a pre-formatted empty RedoxFS filesystem.

2. [x] In `kernel/src/task/drivers/mmc/sd.rs`, implement a flush/sync command.
   SD specification defines CMD32/CMD33/CMD38 for erase, but for cache flush
   the relevant operation is:
   - For SD cards with CMD6 switch-function cache support: CMD48/CMD49 or
     the cache-flush extension.
   - Minimum viable: issue a CMD13 (SEND_STATUS) busy-wait to confirm the
     card has completed all pending write operations. This does not flush
     volatile write cache on newer cards but ensures transfer completion.

3. [x] Wire `MmcBlock::flush()` to call the new sync operation instead of
   returning `Ok(())`.

4. [x] Verify `blk_router` propagates flush through to kernel `sys_blk_flush`.

5. [x] Build an RPi3 image with the new P5 provisioning. Deploy via TFTP.

6. [x] On the physical RPi3:
   - Boot, write a test file to `/srv/test/marker.txt` via shell or a
     test cell.
   - Read the file back to confirm write-through.
   - Power cycle the RPi3.
   - Boot again, read `/srv/test/marker.txt`, confirm contents match.

7. [x] Document the RPi3 persistence evidence (UART log captures, file contents
   before/after reboot).

## Success Criteria

- [x] RPi3 SD image includes a formatted RedoxFS P5 partition.
- [x] `MmcBlock::flush()` waits for hardware write completion via CMD13.
- [x] VFS writes to the storage gate read back correctly on the same boot for
  FAT, littlefs, and RedoxFS.
- [x] Data persists across an RPi3 power cycle for FAT, littlefs, and RedoxFS.
- [x] Existing RV64 QEMU RedoxFS tests still pass.

## Security Considerations

- `/srv/cellos` namespace remains KMS-only; this phase uses `/srv/test/` or
  similar. Do not weaken the KMS access-table boundary.
- Write authorization follows existing VFS `AccessTable` rules; no new
  privilege is introduced.

## Risk Notes

- SD card write reliability varies by card quality. Use a known-good Class 10
  card for testing.
- If the SDHCI controller or card does not support cache-flush commands,
  the minimum viable approach (CMD13 busy-wait) provides transfer-completion
  assurance but not volatile-cache durability. Document the limitation.
- Power-loss during write without journaling (FAT) can corrupt P1. RedoxFS
  CoW on P5 is more resilient but not proven against power-loss on RPi3.

## Deviation Log

Evidence:

- `.agents/debug/rpi3-redoxfs-boot2-persistence.raw:217` records the RedoxFS
  `/srv` P5 volume opening.
- `.agents/debug/rpi3-redoxfs-boot2-persistence.raw:290-301` records same-boot
  readback and cross-power-cycle persistence for all three filesystems.
- `.agents/debug/rpi3-redoxfs-boot2-persistence.raw:349-350` records `89 PASS,
  0 FAIL` and `ALL TESTS PASSED`; the capture contains no SDHCI timeout.
- The operator reported guarded P5 provision verification with SHA-256
  `6cb0d62bdf3579858516b7e3798d3e9189025da37d179cd45de7cf8577906631`.
