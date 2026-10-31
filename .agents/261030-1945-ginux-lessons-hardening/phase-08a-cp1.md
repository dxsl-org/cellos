# Phase 08 — CP1 (Law-1 wire/error delta + slicing)

Status: 08A APPROVED by the user 2026-10-31 ("Duyệt 08A như đề xuất"); implemented. 08B/08C each need their own CP1.

## Census (source-pinned)

Block path:
- Three real block producers, protocol duplicated in each dispatcher (no shared crate): AHCI `cells/drivers/ahci/src/dispatch.rs:3-13`, NVMe `cells/drivers/nvme/src/dispatch.rs:3-12`, virtio-blk `cells/drivers/virtio-blk/src/dispatch.rs:3-13`.
- Wire today: request op 0 read / 1 write / 2 flush; reply `0x00` success (+512 B for read), `0x01` every failure.
- Issue points: AHCI `PxCI` write (`ahci/src/controller.rs:477-488`); NVMe SQ tail doorbell (`nvme/src/controller.rs:429-445`); virtio-blk queue notify.
- Post-issue timeout → generic `ViError::IO` → `0x01` (AHCI `controller.rs:489-514`, NVMe `controller.rs:441-465`). AHCI recovers the port and keeps serving, so a post-issue timeout is indistinguishable from a pre-issue refusal.
- Sole raw consumer `cells/services/vfs/src/blk_router.rs:60-190` reduces the reply to `bool`. `BlockStream::write_raw_sector` replays every failed write 3× (`block_stream.rs:121-150`). Other consumers: `page_cache.rs:55-79`, `backend_fat.rs:39-67,417-419`, `disk_cellosfs.rs:23-52` (`/data`, `/srv`). `durable-charge-journal` has no production binding to this path.
- No block-driver fault-injection seam exists.

Kernel quiescence (driver death):
- Present: deferred retirement funnel (`kernel/src/task.rs:1424-1485`); IOMMU revoke + IOTLB/context invalidation with completion wait and fail-closed retention, on x86 VT-d and RISC-V IOMMU (`task.rs:1820-1882`, `iommu_x86.rs:200-256`, `iommu_riscv.rs:465-512`); BDF/pin release only after acknowledgement.
- Gaps:
  - Never clears bus-master (`pcie_ecam.rs` has only `enable_bus_master`).
  - No FLR/controller reset.
  - No MSI/MSI-X masking (the capability walk records MSI-X/PM but nothing consumes them).
  - IRQ waiter is a bare TID with no generation, no unregister on death and no handler drain (`irq_wait.rs:26-148`).
  - MMIO release does not depend on any quiescence (`resource_registry.rs:548-556`).
  - No device lease or re-grant gate.
- Tests: `ahci-x86` covers bind, persistence and dual driver; there is no driver crash/restart test.

## Proposed slicing (WIP=1)

| Slice | Scope | Law-1 |
|---|---|---|
| **08A** | Typed block outcome end-to-end: shared wire constants; the 3 producers classify by issue point; VFS stops replaying uncertain writes. Witness: test-hooks injection in AHCI (suppress completion after `PxCI`), observe `Indeterminate` at VFS and exactly one `PxCI` write. | **Yes: reply byte semantics** |
| 08B | Kernel retirement quiescence: bus-master disable + MSI/MSI-X mask from the kernel capability walk; IRQ waiter bound to (tid, generation), unregistered on death; MMIO/BDF re-grant gated on IOMMU ack + quiesce; failure → quarantine (retained, charged). | No wire change; semantic change to retirement → CP1 at that time |
| 08C | AHCI controller reset strategy (HBA/port reset per AHCI 1.3.1 §10.4) on restart with a fresh lease; crash/restart integration test with readback and sibling NVMe owner survival; inject failed IOTLB ack → non-reuse. | Possibly (lease) |

## 08A wire delta (to be approved)

Request unchanged. Reply byte 0 for write/flush (and read, for uniformity):

| Byte | Meaning | Caller policy |
|---|---|---|
| `0x00` | Completed | success (unchanged) |
| `0x01` | **NotIssued**: refused before any hardware notification (malformed request, bad LBA, driver uninitialised/poisoned, DMA prep failed). Driver state unchanged. | Retry allowed |
| `0x02` | **Indeterminate**: notified the device (PxCI / doorbell / queue notify), then timeout, device error, or fatal completion. Medium state unknown. | **Write/flush: never replayed automatically**; surface `EIO` to the caller. Read: retry allowed (idempotent). |
| any other | Unknown/legacy | Treat as Indeterminate (fail closed) |

- Device-reported error after completion (ATA TFD.ERR, NVMe status ≠ 0) is classified as Indeterminate for writes: a partial medium write cannot be excluded.
- VFS: `blk_router` returns `BlockOutcome { Completed, NotIssued, Indeterminate }`. The multi-sector helpers stop at the first non-Completed result and return it. `write_raw_sector` retries only `NotIssued`. Page cache, FAT and CellosFS adapters propagate failure as today (all map to `Err`). Behaviour only changes in the retry decision.
- Constants live in a single module (`libs/types` → `block_wire`) and are imported by all 3 drivers and VFS. This removes the triplication.
- The fault-injection seam exists only under the `test-hooks` feature of the AHCI cell; it is not built in production.

Residual: QEMU/TCG only. 08A does not make retirement safe; that is 08B.
