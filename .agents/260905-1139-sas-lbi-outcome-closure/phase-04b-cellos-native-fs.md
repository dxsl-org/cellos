---
phase: "04b"
title: "CellosFS Native & Vector Block DMA Engine"
status: completed
priority: P1
effort: ""
dependencies: [2]
tier: thinking
---

# Phase 04b: CellosFS Native & Vector Block DMA Engine

> Approved on 2026-09-05: Replace external RedoxFS and LittleFS dependencies with an in-tree, pure-Rust, SAS-native, power-loss-resilient filesystem engine (`libs/cellos-fs`). Retain external `fatfs` crate exclusively for `/mnt/sd` interoperability.

## Overview
Unblock Phase 05 (stateful workload checkpoint/readback) and Phase 04 (VFS grant benchmark rows) by building a unified CellosFS Native engine.
Eliminate the C toolchain dependency (`littlefs2-sys` needing `riscv-none-elf-gcc`) and the 10 K LOC external dependency (`third_party/redoxfs`), replacing the 512-byte per-sector IPC lockstep with a multi-sector Vector DMA Grant block protocol.

## Requirements
1. **Vector DMA Grant Block Protocol**:
   - Upgrade `blk_router` and Block Drivers (`driver-virtio-blk`, `driver-nvme`) to support multi-sector DMA grant transfers (`ReadBlocks`, `WriteBlocks`) alongside explicit `Flush`.
   - Eliminate the 8x IPC round-trip amplification for 4 KiB filesystem blocks.
2. **Pure-Rust `no_std` Engine (`libs/cellos-fs`)**:
   - Superblock Ring: Cyclic dual/multi-header with monotonic sequence numbers and CRC32C checksums. A commit only advances the sequence after data and header are acknowledged durable. Power loss rolls back cleanly to the prior valid superblock.
   - Extent-based B-Tree: High performance on NVMe/SSD, low tree depth, CoW snapshot semantics.
   - Small-file inlining: Files < 2 KiB are stored directly inside the Inode without allocating separate data blocks.
   - Partition Bounds Enforcement: `BoundedDisk` guarantees no read/write crosses partition limits (`PART_DATA_SECTORS`, `PART_SRV_SECTORS`).
3. **Power-Cut Simulation & Durability Harness**:
   - Host-based test suite with 10,000 injected power cuts after individual block writes.
   - Zero torn writes or metadata corruption; self-healing without requiring offline fsck.
4. **VFS Integration & Clean Decoupling**:
   - Integrate `CellosFsEngine` into `service-vfs`, mounting both `/data` (Flash/robot profile) and `/srv` (server/PC profile).
   - Retain `fatfs` crate for `/mnt/sd` (external SD card exchange).
   - VFS manages capabilities (`DirTable`, `FileHandleTable`), namespace, quotas, and access control; storage engine manages on-disk structures and extent allocation.
   - VFS releases its service lock during backend block I/O.

## Architecture
```text
+-------------------------------------------------------------------------------+
|                       Cellos VFS (Capability & Router)                        |
+-------------------------------------------------------------------------------+
       |                                                   |
       | (/data, /srv, /tmp, root)                         | (/mnt/sd)
       v                                                   v
+------------------------------------+           +------------------------------+
|          CellosFS Native           |           | External SD Engine           |
| - Superblock Ring (cyclic commit)  |           | - Crate `fatfs` (FAT32)      |
| - CoW Extent B-Tree                |           +------------------------------+
| - SAS Grant Zero-Copy Buffer Pool  |                          |
| - Checksum CRC32C per block        |                          |
+------------------------------------+                          |
       |                                                        |
       +----------------------------+---------------------------+
                                    |
                                    v
                     +------------------------------+
                     | Vector Block Driver (DMA)    |
                     +------------------------------+
```

## Related Files
- Create crate: `libs/cellos-fs/` (`Cargo.toml`, `src/{lib,superblock,allocator,btree,inode,transaction,disk}.rs`).
- Create host tests: `libs/cellos-fs/tests/power_cut_fuzz.rs`, `libs/cellos-fs/tests/persistence_test.rs`.
- Modify: `cells/services/vfs/Cargo.toml` (remove `littlefs2`, remove `redoxfs`, add `libs/cellos-fs`).
- Modify: `cells/services/vfs/src/blk_router.rs` (vector block requests with Grant IDs).
- Modify: `cells/drivers/virtio-blk/src/dispatch.rs` and `cells/drivers/nvme/src/dispatch.rs` (multi-sector DMA dispatch).
- Modify: `cells/services/vfs/src/manager.rs` and `cells/services/vfs/src/backend.rs` (mount CellosFS Native on `/data` and `/srv`).

## Implementation Steps
1. **Vector Block Protocol**: Define `DrvBlockRequest::{ReadBlocks, WriteBlocks, Flush}` in `libs/api` or internal driver IPC, allowing up to 64 KiB per operation via SAS Grants. Update `virtio-blk` and `nvme` dispatchers.
2. **Crate `libs/cellos-fs`**: Implement disk abstraction (`BoundedDisk`), CRC32C, superblock ring, extent allocator, and Inode serialization.
3. **CoW Extent B-Tree**: Implement copy-on-write path traversal, node splitting, extent allocation, and transaction commit.
4. **Host Power-Loss Suite**: Run 10,000 simulated power interruptions during file creation, append, overwrite, truncate, and rename. Verify clean rollback to prior consistent superblock.
5. **VFS Integration**: Implement `FsBackend` for `CellosFsBackend`. Replace `LittlefsBackend` and `RedoxFsBackend` in `VfsManager`.
6. **Verification on QEMU**: Verify boot, file creation, readback, two-boot persistence, and unblock Phase 05.

## Success Criteria
- [X] `libs/cellos-fs` compiles `no_std` for `riscv64gc-unknown-none-elf` without any bare-metal C compiler.
- [X] 10,000 power-cut injection tests pass on host with zero corruption (`cargo test -p cellos-fs`).
- [X] Vector block I/O reduces 4 KiB block read IPC count from 8 to 1 (`blk_read_sectors` / `blk_write_sectors`).
- [X] QEMU boots with `/data` and `/srv` mounted on CellosFS Native; two-boot persistence passes (`riscv64_redoxfs_srv_persistence` and `basic` pass).
- [X] External `/mnt/sd` remains accessible via `fatfs`.
- [X] Phase 05 is unblocked from the missing VFS fixture.
