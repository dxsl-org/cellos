# Native FS /srv — RedoxFS + NVMe

**Goal**: Replace the `/srv` StubBackend with a real persistent filesystem (RedoxFS) backed first
by VirtIO-BLK (G1, immediate) and later by NVMe over PCIe (G2, deferred).

**ADR**: `docs/specs/09b-vfs-native-fs-adr.md` (Accepted 2026-06-11) — RedoxFS chosen.

---

## Phase Status

| # | Phase | Status | Parallel with | Blocks |
|---|-------|--------|---------------|--------|
| 01 | [VirtIO-BLK kernel driver](phase-01-virtio-blk-driver.md) | Complete | 02 | 03 |
| 02 | [RedoxFS no_std fork](phase-02-redoxfs-no-std-fork.md) | Complete | 01 | 03 |
| 03 | [VFS /srv RedoxFS backend](phase-03-vfs-srv-backend.md) | Complete | — | 04 |
| 04 | [Integration test](phase-04-integration-test.md) | Complete | — | — |
| 05 | [PCIe ECAM walker (G2)](phase-05-pcie-ecam.md) | Deferred | — | 06 |
| 06 | [NVMe kernel driver (G2)](phase-06-nvme-driver.md) | Deferred | — | — |

**G1 deliverable** (Phases 01–04): `/srv` reads and writes via RedoxFS on VirtIO-BLK. QEMU CI gate.
**G2 deliverable** (Phases 05–06): Swap block transport to real NVMe once PCIe ECAM exists.

---

## Key Dependencies

- P01 ∥ P02 (independent; both feed P03)
- P03 blocked on both P01 and P02
- P03 has two **Law 1** changes requiring 2× user confirmation before implementation:
  - `BlkWriteAsync` syscall (213) — `libs/api/src/syscall.rs`
  - `MANIFEST_FLAG_PART_SRV` (bit 8) — `libs/api/src/manifest.rs`
- P05 is a standalone G2 prerequisite; P06 blocked on P05

---

## Architecture Summary

```
VFS Cell (/srv)
  └─ FsBackend trait
       └─ RedoxFsBackend (cells/services/vfs/src/backend_redoxfs.rs)
            └─ FileSystem<VicellDisk>  (redoxfs crate, forked)
                 └─ VicellDisk (disk_virtio.rs)
                      ├─ BlkReadAsync  syscall 212  (kernel → VirtIOBlk or NVMe)
                      └─ BlkWriteAsync syscall 213  [NEW — Law 1]

Kernel (unsafe boundary)
  ├─ VirtIoHal impl (hal/arch/riscv/src/virtio_hal.rs)  [P01]
  ├─ VirtIOBlk driver  (kernel/src/task/drivers/blk_virtio.rs)  [P01]
  └─ NvmeBlk driver    (kernel/src/task/drivers/blk_nvme.rs)    [P06]
       └─ requires PCIe ECAM  (kernel/src/task/drivers/pcie_ecam.rs)  [P05]
```

---

## Research Notes

- `virtio-drivers` v0.13: production-quality; used by ArceOS, rCore, Android Cuttlefish.
  `VirtIoHal` in HHDM SAS = ~20 lines (`phys = virt - HHDM_BASE`).
- RedoxFS 0.9.0: no_std core confirmed. Blocker: `libc = "0.2"` must be made optional in fork
  (one-line Cargo.toml patch; `libc::` expected only in std-gated FUSE modules).
  All other deps (aes, argon2, lz4_flex, seahash, xts-mode, bitflags) support no_std.
- NVMe: no production-quality `no_std` crate exists; requires PCIe ECAM first.
  `nvme-oxide` is closest but unverified. G2 only.
- DMA in HHDM SAS: `prp_phys = virt_addr - HHDM_BASE`. No IOMMU needed for QEMU; mandatory
  for real G2 hardware (RISC-V IOMMU spec 2023, ratified; ~3–5 K LOC separate effort).

---

## Disk Layout

For G1 testing, `/srv` uses a **single QEMU disk image** (`disk_srv.img`, 519 MB sparse):
```
-drive file=disk_srv.img,format=raw,if=none,id=hd0 -device virtio-blk-device,drive=hd0
```
P1–P4 regions are zero-filled (FAT/LFS degrade gracefully); P5 at LBA 931_072 holds
64 MB of RedoxFS formatted by `scripts/mksrv-img.sh`.  This matches `PART_SRV_BASE_LBA`
directly — no second VirtIO device or `VicellDisk` changes needed.
