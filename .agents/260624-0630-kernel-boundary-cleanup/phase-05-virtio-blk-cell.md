# Phase 05 — VirtIO Block Driver Cell — ⚠️ DESCOPED (S1 Blocker — G2 Redesign Required)

## Status: DEFERRED to G2

> **Red-team finding S1 (STOP-level):** `kernel/src/loader.rs:spawn_from_path` →
> `kernel/src/loader/early.rs:read_file` → `kernel/src/task/drivers/block::read_sector`
> is called SYNCHRONOUSLY for EVERY `sys_spawn_from_path` call — not just at boot, but
> for every cell spawn at runtime (init, shell `exec`, hotswap). This path cannot IPC into
> a userspace Cell: it runs in syscall context of the caller, cannot yield-and-wait for
> a Cell reply without a complete loader redesign.
>
> **Consequence:** VirtIO Block Cell is NOT migrable under the current loader architecture.
> The "BootFS bootstrap resolves chicken-and-egg" analysis was wrong — the chicken-and-egg
> persists for all runtime `spawn_from_path` calls, not only boot.
>
> **Decision (2026-06-24):** `virtio_blk.rs` stays in kernel for G1. It is QEMU-only
> (real hardware uses the NVMe Driver Cell, already migrated). Keeping it in kernel for
> the QEMU developer target does not violate the spirit of the Kernel Boundary Law for G2.
>
> **G2 path (requires separate plan):** Redesign `spawn_from_path` to go through VFS IPC
> for all runtime spawns (FAT32 ELF loading delegated to VFS service). Kernel loader
> retains only the BootFS path for the four embedded Cells (init, platform, virtio-blk,
> vfs). This is a significant loader architecture change and is out of scope for G1.

## Context Links
- Plan: [plan.md](plan.md)
- Source (stays in kernel): `kernel/src/task/drivers/virtio_blk.rs` (193) + `virtio_pci.rs` (276)
- Loader blocker: `kernel/src/loader/early.rs:52,80,131` → `block::read_sector`

## Overview
- **Priority:** Deferred — G2 milestone.
- **Status:** DESCOPED
- **Risk:** The original plan's "Spike B — determine which kernel callers remain" was the right
  instinct, but the answer is: the core loader, on every spawn, forever. Not a spike question —
  a fundamental constraint.

## G2 Prerequisites (before this can be re-planned)

1. **Loader redesign**: `spawn_from_path` must become async or IPC-based for FAT32 reads.
   Embed {init, platform, virtio-blk, vfs} in BootFS; all else loaded by VFS over IPC.
2. **snapshot.rs** has the same dependency (`block::read_sector` for warm-boot restore). Both
   must be redesigned together.
3. **Law 1 impact**: redesigning the loader changes the spawn syscall contract — 2× confirm gate.

## What still happens in G1

- `virtio_blk.rs` remains in kernel; serves both the kernel loader and VFS (via `sys_blk_read`).
- `virtio_net.rs` migration (Phase 06) is INDEPENDENT of this descoping — net does not use the
  loader path and can proceed. See phase-06.
- Phase 08 (cleanup) will NOT delete `virtio_blk.rs` or `virtio_pci.rs`. Update Phase 08 accordingly.
- Phase 07 (MMC Cell) is also affected: same `block::read_sector` dependency. See phase-07.

## Key Insights (verified + decisions)
- VFS already speaks the `DrvRequest` protocol to whatever registers `service::BLOCK_DRIVER` (9). NVMe + e1000 proved it. VirtIO blk Cell must serve the **identical wire format**: read `[op=0][sector u64]` (10B) → `[0x00][512B]` (513B); write `[op=1][sector u64][512B]` (522B) → `[0x00]` (1B); error `[0x01]`. (`nvme/src/dispatch.rs`).
- **Bootstrap order:** kernel → Platform Cell (BootFS) → **VirtIO Block Cell (BootFS)** → VFS (BootFS) → config → rest from FAT32. VFS, once it can read FAT32 via the blk Cell, serves `/bin/*` to init for everything else.
- The kernel currently has `BLOCK_DEVICE: Spinlock<Option<SafeVirtIOBlk>>` and a `viVirtIOBlk: ViBlockDevice` impl used by `block.rs:block_device()`. After migration, `block_device()` must route to the **registered Cell** (via `sys_lookup_service(BLOCK_DRIVER)`), with the kernel `viVirtIOBlk` retained as fallback only until Phase 08.
- **`virtio-drivers` crate HAL in a Cell** — OPEN QUESTION from plan.md #2. The kernel uses `virtio_hal.rs` (`VirtioHal` impl) + `virtio_common.rs`. A Cell cannot use the kernel's HAL. Options: (a) the Cell implements its own `Hal` trait using `ostd::dma::DmaBuf` for queue memory + `ostd::mmio` for notify — this needs a small documented `unsafe` island for the `virtio-drivers` `Hal::dma_alloc`/`mmio_phys_to_virt` contract (Law 4 exception, like NVMe). (b) write a minimal hand-rolled VirtIO blk queue driver in the Cell (no external crate) — more code, fully `#![forbid(unsafe_code)]` except MMIO. **Spike decides; (a) preferred to reuse the proven crate.**
- **x86_64 PCI path** (`virtio_pci.rs`): on q35, the device is found via `sys_find_pcie_device(vendor 0x1AF4 → use class match)` after Platform Cell registers BARs. The Cell gets the BAR, constructs an MMIO transport from it (PCI capability structures point to BAR+offset for the VirtIO common/notify/device cfg). This logic ports from `virtio_pci.rs` into the Cell.

## Requirements
### Functional
1. `cells/drivers/virtio-blk/` Cell: MMIO transport (RISC-V/ARM64) + PCI transport (x86_64); drive blk virtqueue; serve `DrvRequest`; `sys_register_block_driver()`.
2. Embed in BootFS; kernel spawns it before VFS, grants `PcieDriverCap` (path `/bin/virtio-blk`).
3. IRQ: on RISC-V/ARM64 MMIO, `sys_wait_irq(slot_irq)` to sleep until the used ring advances; x86_64 PCI polls (no MSI-X) — same as NVMe/e1000 today.
4. `block.rs:block_device()` routes to the registered Cell; kernel `viVirtIOBlk` is fallback only.

### Non-Functional
- Cell `#![forbid(unsafe_code)]` except the VirtIO HAL/MMIO island (documented `// SAFETY:`).
- Owned buffers (Law 2): the 512B sector buffer in the IPC reply is a stack array reused per request (matches NVMe `[0u8; REPLY_SIZE]`), not a borrowed slice across an await.
- Boot must still reach `Cellos>` shell reading FAT32 rootfs.

## Architecture

### Bootstrap sequence (critical)
```
1. Kernel boot, IOMMU init, scheduler up
2. spawn_from_mem(PLATFORM_ELF)  → PlatformCap   (Phase 01; x86_64 registers VirtIO BARs)
3. spawn_from_mem(VIRTIO_BLK_ELF) → PcieDriverCap
      Cell Init:
        RISC-V/ARM64: probe VirtIO MMIO slots (claim via sys_request_mmio), find Block type
        x86_64:       sys_find_pcie_device(VirtIO blk class) → BAR → PCI transport
        init virtqueue (HAL via ostd::dma)
        sys_register_block_driver()                # registers service::BLOCK_DRIVER=9
4. spawn_from_mem(VFS_ELF)
      VFS Init: sys_lookup_service(BLOCK_DRIVER) → blk Cell TID
                reads FAT32 superblock via DrvRequest IPC → mounts rootfs
5. init (already running or spawned next) → spawns /bin/* from FAT32 via VFS
```

> Chicken-and-egg resolved: blk Cell + VFS + platform + init all live in **BootFS** (embedded
> `include_bytes!`), so none of them require disk to load. Everything *else* comes from FAT32.

### IPC data flow (steady state)
```
VFS                          virtio-blk Cell                  Hardware
---                          ---------------                  --------
blk_read(lba) →
  sys_send([0,lba], 10B) ───► AppEvent::Message
                              parse op=0, sector=lba
                              dev.read_blocks(sector, &mut buf)  ──► virtqueue
                              (RISC-V: sys_wait_irq if not ready)  ◄── used ring + IRQ
                              reply = [0x00] ++ buf(512)
  ◄── sys_recv 513B ─────────  sys_send(sender, reply)
```

### IRQ handling
- **RISC-V/ARM64 MMIO:** Cell thread blocks in `sys_wait_irq(slot_irq)`; kernel ISR (Phase 00 `irq_wait::wake_irq`) acks the device's `InterruptStatus` + PLIC and wakes the Cell. Cell then drains the used ring. The kernel's old `vi_handle_virtio_irq` block-device branch is removed (Phase 08); the ack now happens in the Cell after wake — BUT the PLIC ack must stay in kernel ISR (privileged). **Design:** kernel ISR acks PLIC + reads device InterruptStatus to ack the device line (minimal), then wakes Cell; Cell processes the queue. Confirm device-InterruptStatus ack can be done generically in kernel without device-specific knowledge (VirtIO InterruptStatus offset is standard) — or Cell acks device, kernel acks PLIC only. **Spike decides.**
- **x86_64 PCI:** polled (no MSI-X wired), identical to NVMe/e1000 — no `sys_wait_irq`.

## Related Code Files
**Create:**
- `cells/drivers/virtio-blk/Cargo.toml` (deps: types, api, ostd, virtio-drivers — confirm crate builds for the cell target), `build.rs`, `src/main.rs`, `src/dispatch.rs` (copy nvme protocol), `src/hal.rs` (VirtIO Hal via ostd::dma — the unsafe island), `src/transport.rs` (MMIO + PCI transport selection).

**Modify:**
- `kernel/src/main.rs` — `static VIRTIO_BLK_ELF = include_bytes!`; spawn after Platform, before VFS; grant PcieDriverCap.
- `kernel/src/loader.rs` — add `/bin/virtio-blk` to PcieDriverCap path-grant list (with nvme, e1000).
- `kernel/src/task/drivers/block.rs` — `block_device()` routes via `sys_lookup_service(BLOCK_DRIVER)` to the Cell; kernel `viVirtIOBlk` becomes fallback. (NOTE: block.rs is in-kernel; it cannot call userspace `sys_lookup_service` — instead it reads `driver_cell::BLOCK_DRIVER_CELL` AtomicUsize. When non-zero, in-kernel callers of `read_sector` must route through IPC to that TID. **This is the subtle part:** the kernel's own early boot block reads (before VFS) must go through the Cell too. Decision: kernel early-boot block reads use a kernel→Cell IPC helper OR the BootFS-embedded cells avoid needing kernel block reads at all. Confirm what in-kernel code still calls `block::read_sector` after VFS owns the disk.)
- `gen_disk.ps1` + embedded — build `-p driver-virtio-blk`, sign, embed.
- root `Cargo.toml` — member.

## Implementation Steps
1. **Spike A — HAL in Cell:** prototype `virtio-drivers` `Hal` impl in the Cell using `ostd::dma::DmaBuf` (phys==virt in SAS). Confirm `dma_alloc`/`dma_dealloc`/`mmio_phys_to_virt`/`share`/`unshare` satisfiable. If `virtio-drivers` won't build in the cell sandbox, fall to hand-rolled queue. **Output: HAL works yes/no.**
2. **Spike B — kernel in-kernel block reads:** grep every caller of `block::read_sector`/`block_device()` in kernel. Determine which still run after the blk Cell registers (snapshot.rs? early loader?). If the kernel itself needs block I/O post-migration, design a kernel→Cell IPC shim. If NOT (VFS owns all disk I/O), `block.rs` only matters during the BootFS transition window.
3. Scaffold `cells/drivers/virtio-blk/` from nvme template.
4. Port MMIO transport probe (`virtio_blk.rs:init_driver` slot loop) into `src/transport.rs`, claiming each slot via `sys_request_mmio`.
5. Port x86_64 PCI transport (`virtio_pci.rs` BAR→MmioTransport construction) into `src/transport.rs` behind `cfg(target_arch="x86_64")`, using `sys_find_pcie_device`.
6. `src/dispatch.rs`: copy nvme's `[op][sector]`→reply protocol exactly (VFS already speaks it).
7. Init handler: select transport by arch → init `VirtIOBlk` → `sys_register_block_driver()`.
8. IRQ: RISC-V/ARM64 spawn a wait loop using `sys_wait_irq(slot_irq)`; x86_64 poll. (Per Spike on kernel-side ack.)
9. Kernel `main.rs`: embed + spawn (after Platform, before VFS); PcieDriverCap grant.
10. `block.rs`: route to `BLOCK_DRIVER_CELL` TID when set; keep `viVirtIOBlk` fallback.
11. `gen_disk.ps1` + embedded copy + root Cargo.toml.
12. **Boot test RISC-V virt:** kernel → platform → virtio-blk Cell → VFS mounts FAT32 → init spawns shell → `Cellos>` + `cat`/`ls` work. This is the make-or-break test.
13. **Boot test x86_64 q35:** PCI path; same outcome.
14. Negative test: rename `/bin/virtio-blk` so it fails to load → kernel fallback `viVirtIOBlk` still boots (rollback proof).

## Todo List
- [ ] Spike A: virtio-drivers HAL in Cell
- [ ] Spike B: enumerate in-kernel block::read_sector callers post-migration
- [ ] Scaffold virtio-blk cell
- [ ] MMIO transport probe (RISC-V/ARM64)
- [ ] PCI transport (x86_64)
- [ ] dispatch.rs (copy nvme protocol)
- [ ] Init handler + register_block_driver
- [ ] IRQ wait loop (RISC-V/ARM64) + kernel ack design
- [ ] kernel main.rs BootFS embed + spawn order + cap
- [ ] block.rs route-to-Cell + fallback
- [ ] gen_disk sign/embed + Cargo member
- [ ] RISC-V boot-to-shell-from-FAT32 test
- [ ] x86_64 boot test
- [ ] fallback rollback test

## Success Criteria
- [ ] RISC-V virt boots to `Cellos>` with rootfs served by the virtio-blk Cell (kernel `BLOCK_DEVICE` static unused for I/O).
- [ ] `cat /etc/...`, `ls /bin` read real FAT32 content through the Cell.
- [ ] `service::BLOCK_DRIVER` resolves to the Cell TID; VFS uses IPC path (logged).
- [ ] x86_64 q35 boots via PCI transport.
- [ ] Disabling the Cell (rename) → kernel fallback boots (no regression vs today).
- [ ] Reboot persistence: writes flush through the Cell's VirtIO FLUSH (matches `virtio_blk.rs:flush`).

## Risk Assessment
| Risk | L | I | Mitigation |
|------|---|---|-----------|
| `virtio-drivers` crate won't build in Cell sandbox | Med | High | Spike A first; fallback = hand-rolled minimal queue driver |
| Boot deadlock: VFS waits for blk Cell that waits for something VFS provides | Low | Crit | Strict BootFS order; blk Cell needs ZERO disk/VFS; only MMIO + DMA |
| Kernel still needs block I/O post-handoff (snapshot, early loader) | Med | High | Spike B enumerates callers; add kernel→Cell IPC shim or keep fallback for those paths |
| IRQ ack split (kernel PLIC vs Cell device) races into storm | Med | High | Kernel ISR acks PLIC + device InterruptStatus (standard offset) then wakes; never leave line asserted |
| x86_64 PCI BAR not registered yet when Cell inits | Med | High | Platform Cell (P01) spawns first; blk Cell retries find_pcie_device with bounded backoff |
| Performance regression (IPC per sector vs in-kernel call) | Med | Med | Same overhead NVMe already accepted; PageCache (4MB LRU) amortizes; measure vs baseline |

## Security Considerations
- VirtIO MMIO/BAR claimed exclusively via `sys_request_mmio` + BDF ownership in resource_registry — no other Cell can touch the disk hardware.
- DMA buffers authorized to the device BDF via IOMMU (`DmaBuf::authorize`) — device cannot DMA outside the Cell's granted buffers (the whole point of the IOMMU-in-kernel whitelist item).
- A compromised blk Cell can corrupt the filesystem but cannot escape its MMIO/DMA grant or touch other Cells' memory (LBI + IOMMU).

## Next Steps
- Establishes the block-register + BootFS pattern reused by Phase 06 (net) and Phase 07 (mmc).
- Phase 08 removes kernel `virtio_blk.rs`, `virtio_pci.rs`, and the `viVirtIOBlk` fallback in `block.rs`.
