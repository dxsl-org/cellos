---
title: "Kernel Boundary Law Cleanup — Driver Cell Migration"
description: "Migrate remaining VirtIO/MMC/PCIe drivers + console code out of kernel into userspace Driver Cells per the Kernel Boundary Law."
status: pending
priority: P1
effort: 9 phases (~28-36 dev-days)
branch: main
tags: [kernel-boundary, driver-cells, virtio, pcie, mmc, sas, lbi, migration]
created: 2026-06-24
---

# Kernel Boundary Law Cleanup — Driver Cell Migration

> Ratified law: `docs/specs/15-kernel-boundary.md` (2026-06-23). After NVMe + e1000 + hotswap
> migration (commit c70d8273), the kernel still hosts VirtIO device drivers, the PCIe ECAM
> scanner, the MMC stack, and framebuffer/console driver code — all explicitly **blacklisted**.
> This plan exiles them into Driver Cells / Platform Cell / the Input service.

## North Star

Every migration here **enforces SAS/LBI** — it is not ecosystem-chasing. Driver Cells are the
canonical demonstration of the kernel-boundary bargain: the kernel keeps only IOMMU init +
capability enforcement + IRQ dispatch primitive; the *device logic* lives in `#![forbid(unsafe_code)]`
(except MMIO) userspace Cells, isolated by Rust's type system, reachable only via zero-copy IPC.

The reference is **already proven**: `cells/drivers/nvme/` and `cells/drivers/e1000/` migrated
successfully. Every phase below follows that exact pattern (find/claim MMIO → register driver →
serve `DrvRequest` IPC). We are *replicating a validated pattern*, not inventing one.

## Verified Baseline (re-grepped 2026-06-24, do not trust without re-checking)

| Fact | Value | Source |
|------|-------|--------|
| Reference Block Driver Cell | `cells/drivers/nvme/` (src/main.rs, dispatch.rs) | exists |
| Reference NIC Driver Cell | `cells/drivers/e1000/` | exists |
| `RegisterBlockDriver` syscall | **416** | `libs/api/src/syscall.rs` |
| `RegisterNicDriver` syscall | **417** | `libs/api/src/syscall.rs` |
| `FindPcieDevice` syscall | **418** | `libs/api/src/syscall.rs` |
| `RequestMmio` syscall | **213** | `libs/api/src/syscall.rs:171` |
| `GrantDma` syscall | **233** | `libs/api/src/syscall.rs:65` |
| **Highest assigned syscall** | **420** (Snapshot) | `libs/api/src/syscall.rs:72` |
| **Free dense gap** | **234–255** | between GrantDma(233) and hypervisor block (220-227 taken) |
| `PcieDriverCap` ZST | `kernel/src/task/cap.rs:72` granted by `/bin/nvme`,`/bin/e1000` path match in `loader.rs:300` | exists |
| `SupervisorCap` ZST | `kernel/src/task/cap.rs:59` granted by `/bin/supervisor` | exists |
| Driver-cell registry | `kernel/src/task/drivers/driver_cell.rs` (BLOCK_DRIVER_CELL / NIC_DRIVER_CELL AtomicUsize) | exists |
| Block routing | `kernel/src/task/drivers/block.rs:9` `block_device()` | exists |
| `service::BLOCK_DRIVER` | 9 | `libs/api/src/syscall.rs` |
| `service::NIC_DRIVER` | 10 | `libs/api/src/syscall.rs` |
| Cell VA model | PIE / ET_DYN, dynamic VA from `0x1_0000_0000`, 32 MiB stride | `kernel/src/loader/va_alloc.rs:27` |
| Embedded-cell pattern | `INIT_ELF = include_bytes!`, `spawn_from_mem` | `kernel/src/main.rs:69,507` |
| init spawn order | vfs→config→input→net→compositor→silo→net-broker→supervisor→shell | `cells/tools/init/src/main.rs:60` |
| Cell signing | ed25519, `__ViCell_sig` via objcopy, in `gen_disk.ps1` + `scripts/sign-cell.py` | exists |

### CORRECTION vs task brief

The brief suggested new syscalls "~220+/~221+". **Those numbers are TAKEN** (220-227 = hypervisor
ops CreateVm..ReadGuestMemory). The correct free dense gap is **234-255**. This plan assigns:
- `sys_wait_irq` = **234**
- `sys_register_pcie_bar` = **235**

## Syscall Number Assignments (Phase 00 — Law 1 gated)

| Name | Number | Allowlist bit (next free) | Cap required |
|------|--------|---------------------------|--------------|
| `WaitIrq` | 234 | bit 51 | `PcieDriverCap` (or `PlatformCap`) |
| `RegisterPcieBar` | 235 | bit 52 | `PlatformCap` (new) |

> **LAW 1 GATE**: adding these to `libs/api/src/syscall.rs` requires **2× user confirmation**.
> Phase 00 must not proceed past Step 2 without it. See phase-00.

## Red Team Outcomes (2026-06-24 — CAUTION verdict)

> Run before implementation. Two STOP-level issues resolved; plan updated accordingly.

| Issue | Severity | Resolution |
|-------|----------|-----------|
| **S1** — `spawn_from_path → block::read_sector` called on every spawn; VirtIO Block Cell can't IPC from kernel loader context | STOP | Phase 05 **DESCOPED** to G2. `virtio_blk.rs` stays in kernel for G1 (QEMU-only; real hardware uses NVMe Driver Cell). |
| **S2** — Phase 00 ISR diagram showed "ISR wakes tid (Ready)" which requires SCHEDULER lock in ISR — deadlock | STOP | Phase 00 ISR diagram rewritten: ISR only sets `IRQ_PENDING[irq]=true` (atomic, no lock); scheduler sweep does the actual Ready transition. Model: `waker.rs:consume_pending`. |
| **H1** — IRQ ack split: kernel must ack VirtIO InterruptStatus or interrupt storm | HIGH | Phase 00 diagram updated: ISR acks both PLIC and VirtIO InterruptStatus offset 0x60 before setting IRQ_PENDING. Kernel retains this narrow VirtIO register knowledge. |
| **H2** — Shared/duplicate IRQ waiters unhandled | HIGH | Policy: single-waiter per IRQ, second caller gets `AlreadyClaimed`. x86_64 PCI polls (no `sys_wait_irq`) so INTx sharing is irrelevant. Documented in phase-00. |
| **H3** — Platform Cell holds ECAM write capability for lifetime → can reprogram any device's BAR including IOMMU | HIGH | Phase 01 updated: Platform Cell MUST relinquish ECAM MMIO claim via `Drop(MmioRegion)` after enumeration (one-shot scan semantics). |
| **M3** — Cell crash mid-I/O leaves device writing into freed DMA frame | MED | Add to phase-08 cleanup: `VirtIO DEVICE_RESET` before `Drop` releases DMA frames. |

## Dependency Graph

```
Phase 00 (syscalls 234/235 + PlatformCap + user_hello gate)   [GATE: Law 1 ×2]
    │
    ├──> Phase 01 (Platform Cell: PCIe ECAM — x86_64 only)    [needs 00; parallel-eligible]
    │
    ├──> Phase 02 (VirtIO GPU Driver Cell)                     [needs 00; parallel]
    ├──> Phase 03 (VirtIO Input → input service)               [needs 00; parallel]
    └──> Phase 04 (VirtIO Sound Driver Cell)                   [needs 00; parallel]
    │
    └──> Phase 06 (VirtIO Net Driver Cell)                     [needs 00; independent of 05]
    │
    └──> Phase 07 (MMC Cell)                                   [needs 00; also DESCOPED — see note]
    │
    └──> Phase 08 (kernel cleanup: remove migrated code)       [needs 01+02+03+04+06 green]

Phase 05 (VirtIO Block Cell) — DESCOPED to G2 (S1: loader architecture blocker)
Phase 07 (MMC Cell) — review: same block::read_sector dependency as Phase 05; likely also G2.
```

**Parallelizable after Phase 00:** {01}, {02}, {03}, {04}, {06} touch disjoint files.
Phase 07 (MMC) needs a separate architecture review before scheduling — MMC also serves the
kernel loader path. Phase 08 runs last after all active phases pass boot green.

## File-Ownership Map (no two parallel phases touch the same file)

| Phase | Owns (create/modify) |
|-------|----------------------|
| 00 | `libs/api/src/syscall.rs`, `libs/ostd/src/syscall.rs`, `kernel/src/task/syscall.rs` (new arms), `kernel/src/task/cap.rs`, `kernel/src/loader.rs` (path grant), `kernel/src/task/user_hello.rs` |
| 01 | `cells/services/platform/**` (new), `kernel/src/main.rs` (spawn platform), `kernel/src/task/drivers/pcie_ecam.rs` (shim) |
| 02 | `cells/drivers/virtio-gpu/**` (new), `kernel/src/main.rs` (spawn) — fb_console removal deferred to P08 |
| 03 | `cells/services/input/**` (extend), `kernel/src/main.rs` (cap grant) |
| 04 | `cells/drivers/virtio-sound/**` (new) |
| 05 | `cells/drivers/virtio-blk/**` (new), `kernel/src/main.rs` (BootFS embed+spawn), `kernel/src/task/drivers/block.rs` |
| 06 | `cells/drivers/virtio-net/**` (new), `kernel/src/task/drivers/nic.rs` |
| 07 | `cells/drivers/mmc/**` (new), `kernel/src/task/drivers/block.rs` (MMC fallback removal coord w/ P05) |
| 08 | DELETE: virtio_*.rs, pcie_ecam.rs, mmc*, fb_console.rs, input_map.rs; SIMPLIFY console_drv.rs |

> Conflict watch: P05, P07, P08 all touch `block.rs`. P05 owns block.rs edits; P07 coordinates
> through P05's final shape; P08 only deletes after both land. `kernel/src/main.rs` spawn edits
> are append-only additions in distinct regions — serialize merges, but logically disjoint.

## Phases

| # | File | Title | Status | Effort | Risk |
|---|------|-------|--------|--------|------|
| 00 | [phase-00-prerequisites.md](phase-00-prerequisites.md) | Syscalls + PlatformCap + test gate | **complete** | 3-4d | **HIGH** (Law 1, kernel blocking primitive) |
| 01 | [phase-01-platform-cell-ecam.md](phase-01-platform-cell-ecam.md) | Platform Cell (PCIe ECAM) | **complete** | 4-5d | HIGH (x86_64 boot dependency) |
| 02 | [phase-02-virtio-gpu-cell.md](phase-02-virtio-gpu-cell.md) | VirtIO GPU Driver Cell | **complete** | 3d | MED |
| 03 | [phase-03-virtio-input-cell.md](phase-03-virtio-input-cell.md) | VirtIO Input → input service | **complete** | 3d | MED |
| 04 | [phase-04-virtio-sound-cell.md](phase-04-virtio-sound-cell.md) | VirtIO Sound Driver Cell | **YAGNI-delete** (P08) | — | LOW |
| 05 | [phase-05-virtio-blk-cell.md](phase-05-virtio-blk-cell.md) | ~~VirtIO Block Cell~~ | **DESCOPED** | G2 | S1 loader blocker |
| 06 | [phase-06-virtio-net-cell.md](phase-06-virtio-net-cell.md) | VirtIO Net Driver Cell | **complete** | 2-3d | MED |
| 07 | [phase-07-mmc-cell.md](phase-07-mmc-cell.md) | ~~MMC Storage Driver Cell~~ | **DESCOPED** | G2 | same S1 risk on real HW; QEMU has no SDHCI |
| 08 | [phase-08-cleanup.md](phase-08-cleanup.md) | Remove migrated kernel driver code | **pending — awaiting 3-arch boot verify** | 2d | LOW (additive-revert safe) |

## Cross-Cutting Constraints

- **Law 1**: any `libs/api/` edit (syscall numbers, manifest, PcieDeviceInfo) needs 2× user confirm. Phase 00 + any phase adding `PcieBarInfo`.
- **Law 2**: async IPC handlers use `Box<[u8]>`, never `&mut [u8]`.
- **Law 4**: Driver Cells `#![forbid(unsafe_code)]` — MMIO goes through `ostd::mmio::MmioRegion` (already safe-wrapped, bounds-checked).
- **Law 5**: no `mod.rs`; `foo.rs` parallel to `foo/`.
- **Law 8**: each Cell impls `Drop` to release MMIO claim + deregister driver role on exit.
- Every new cell: `ostd::run_app!(handler)`, `declare_manifest!`, workspace member in root `Cargo.toml`, signed in `gen_disk.ps1`, spawned by init (or BootFS for blk/platform).

## Global Rollback Strategy

Each Driver Cell migration is **additive + gated**: the kernel-resident driver stays in place
until Phase 08. `block_device()` / `nic.rs` route to the registered Cell **only if a Cell
registered** (TID != 0); otherwise fall back to the kernel driver. So at every phase 01-07, if the
new Cell fails to register or crashes, the kernel driver still serves I/O. Rollback = "don't spawn
the Cell" (one line in init / main.rs). Phase 08 is the only destructive step and runs last, behind
a full green boot of all migrations.

## Success Criteria (whole plan)

- [ ] `kernel/src/task/drivers/` contains NO device driver logic (only: registry.rs, driver_cell.rs, block.rs/nic.rs routers, gpio_irq.rs IRQ dispatch, uart.rs early stub, iommu*, virtio_hal.rs/virtio_common.rs if still needed by cells via shared crate, ramdisk.rs)
- [ ] QEMU RISC-V virt boots to `Cellos>` shell with VirtIO blk Cell serving the rootfs
- [ ] QEMU x86_64 q35 boots with Platform Cell scanning PCIe + VirtIO blk/net Cells via PCI BAR
- [ ] `cat`, `ls`, network ping, GPU compositor, keyboard input all functional through Cells
- [ ] `grep -rn "VirtIOBlk\|VirtIONet\|pcie_ecam::init\|fb_console" kernel/src` returns only the deleted-file-free tree
- [ ] All Driver Cells signed + spawned + register their role; `sys_lookup_service` resolves each

## Open Questions

1. **IRQ on x86_64 / PCI MSI-X**: NVMe/e1000 PCI path runs *polled* today (no MSI-X). Does `sys_wait_irq` need MSI-X wiring for VirtIO-PCI, or do VirtIO-PCI Cells also poll? (Decision in Phase 05/06: poll on x86_64, IRQ-block on RISC-V MMIO. Confirm acceptable latency.)
2. **virtio_hal.rs / virtio_common.rs**: kernel-side DMA HAL for `virtio-drivers` crate. Cells need their own HAL using `ostd::dma`. Does the `virtio-drivers` crate work under `#![forbid(unsafe_code)]` in a Cell, or does the HAL impl require a tiny documented unsafe MMIO island? (Investigate in Phase 05 spike.)
3. **Sound**: is there a consumer of VirtIO sound today (any audio Cell/app), or is `virtio_sound.rs` dead code that should just be deleted rather than migrated? (Resolve at start of Phase 04 — may collapse to a delete.)
4. **MMC real-hardware test**: QEMU uses VirtIO, not SDHCI. MMC Cell's SDHCI PIO path is only exercisable on board-rpi4/vf2. Phase 07 validation is QEMU-VirtIO-MMC-emulation OR deferred hardware test — pick one.
