# Phase 07 — MMC Storage Driver Cell

## Context Links
- Plan: [plan.md](plan.md) · Prereqs: [phase-00](phase-00-prerequisites.md), [phase-05](phase-05-virtio-blk-cell.md) (block-register pattern)
- Source: `kernel/src/task/drivers/mmc.rs` + `mmc/*` (~800 LOC)
- Reference Cell: `cells/drivers/nvme/` + `cells/drivers/virtio-blk/` (Phase 05)
- Kernel router: `kernel/src/task/drivers/block.rs:18` `MmcBlock` fallback
- Memory ref: MMC plan — QEMU=VirtIO, real board=SDHCI PIO; board-rpi4 feature flag (0xFE340000); SDHCI on vf2

## Overview
- **Priority:** P2 (after Phase 05).
- **Status:** **DESCOPED to G2** (2026-06-24) — same S1 blocker as Phase 05 on real hardware; on QEMU there is no SDHCI so the Cell would exit immediately with no benefit for G1.
- **Risk:** MED — primarily a **hardware-test** problem: QEMU uses VirtIO, not SDHCI, so the MMC Cell's real path is only exercisable on board-rpi4 / VisionFive2. QEMU validation is limited.
- **Description:** Migrate the MMC/SDHCI block driver (~800 LOC across `mmc.rs` + `mmc/*`) to `cells/drivers/mmc/`. Registers as block driver (same `service::BLOCK_DRIVER` IPC as virtio-blk/nvme). Board-specific MMIO bases via cell feature flags.

## Key Insights (verified)
- `mmc.rs` provides `MmcBlock: ViBlockDevice` used as the 2nd-priority kernel block device (`block.rs:7,18`). After migration it becomes a Driver Cell registering `BLOCK_DRIVER`.
- ~800 LOC is the largest single driver migration here. It spans SDHCI register programming + command/data PIO. All MMIO → `ostd::mmio`.
- **Board feature flags:** the cell needs `board-rpi4` (SDHCI 0xFE340000) / `board-vf2` features mirroring the kernel's. These select the MMIO base. On QEMU virt there's no SDHCI → the Cell finds nothing, exits, VirtIO-blk Cell serves disk.
- **Single block slot:** `BLOCK_DRIVER_CELL` is one AtomicUsize. virtio-blk and mmc cannot both register. On a real board you'd spawn MMC (not virtio-blk); on QEMU you spawn virtio-blk. **Decision:** init spawns the correct block Cell per platform (build-time feature or runtime board detect). Document mutual exclusion.

## Requirements
### Functional
1. `cells/drivers/mmc/`: claim SDHCI MMIO (board-specific base), init MMC card, serve `DrvRequest` block IPC, `sys_register_block_driver()`.
2. Board feature flags select MMIO base; absent hardware → clean exit.
3. Mutually exclusive with virtio-blk for the single block slot — init picks one.

### Non-Functional
- `#![forbid(unsafe_code)]` except SDHCI MMIO island; Law 2 owned sector buffers.
- Must not regress QEMU boot (where MMC is absent and virtio-blk serves disk).

## Architecture
```
VFS → DrvRequest [op][sector] → mmc Cell → SDHCI cmd/data PIO → SD/eMMC card
                                (board-rpi4: base 0xFE340000; board-vf2: <base>)
QEMU virt: no SDHCI → mmc Cell exits at Init → virtio-blk Cell owns BLOCK_DRIVER
```

## Related Code Files
**Create:** `cells/drivers/mmc/` (Cargo.toml with `board-rpi4`/`board-vf2` features, build.rs, src/main.rs, src/sdhci.rs, src/card.rs, src/dispatch.rs (copy block protocol from virtio-blk)).
**Modify:**
- `kernel/src/task/drivers/block.rs` — remove `MmcBlock` fallback once Cell proven (coordinate with Phase 05's block.rs edits; P05 owns the file, P07 lands its mmc-removal after).
- `kernel/src/loader.rs` — `/bin/mmc` PcieDriverCap grant.
- `cells/tools/init/src/main.rs` — spawn mmc OR virtio-blk per board (mutual exclusion logic).
- `gen_disk.ps1` + root Cargo.toml.

## Implementation Steps
1. Decide validation target: QEMU SDHCI emulation (if feasible) OR real board (rpi4/vf2) OR defer hardware test with QEMU-VirtIO-only regression (Open Question plan.md #4). Recommend: port now, gate hardware validation as a follow-up, ensure QEMU boot unaffected.
2. Scaffold from virtio-blk template; add board feature flags.
3. Port `mmc.rs` + `mmc/*` SDHCI init + cmd/data PIO into `src/sdhci.rs` + `src/card.rs` (ostd::mmio).
4. `src/dispatch.rs`: copy the block `[op][sector]` protocol (identical to virtio-blk/nvme).
5. Init: select SDHCI base by feature → init card → `sys_register_block_driver()`; clean-exit if no hardware.
6. init: spawn mmc on board builds, virtio-blk on QEMU (mutual exclusion).
7. block.rs: remove MmcBlock fallback (after P05).
8. gen_disk + member.
9. Validate: QEMU virt boots normally (mmc Cell exits, virtio-blk serves). Board test (rpi4/vf2) reads SD card — if hardware available.

## Todo List
- [ ] Pick validation target (QEMU/board/deferred)
- [ ] Scaffold + board features
- [ ] Port SDHCI init + PIO → sdhci.rs/card.rs
- [ ] dispatch.rs (copy block protocol)
- [ ] Init select-base + register / clean-exit
- [ ] init mutual-exclusion spawn (mmc vs virtio-blk)
- [ ] block.rs remove MmcBlock (after P05)
- [ ] gen_disk + member
- [ ] QEMU boot regression + (optional) board SD test

## Success Criteria
- [ ] QEMU virt boots unchanged (mmc Cell exits cleanly; virtio-blk owns disk).
- [ ] On board-rpi4/vf2 (if tested): mmc Cell reads SD card, registers `BLOCK_DRIVER`, VFS mounts rootfs.
- [ ] Mutual exclusion documented + enforced (only one block Cell registers).
- [ ] kernel `MmcBlock` removed from `block.rs` with no QEMU regression.

## Risk Assessment
| Risk | L | I | Mitigation |
|------|---|---|-----------|
| No QEMU SDHCI → can't validate the real path in CI | High | Med | Accept QEMU-clean-exit regression as the CI gate; hardware test as follow-up |
| Largest driver (~800 LOC) port introduces bugs | Med | Med | Port verbatim into ostd::mmio; diff register sequences against kernel original |
| Both mmc + virtio-blk register → slot conflict | Med | High | init spawns exactly one per platform; Cell exits if hardware absent |
| board feature flag mismatch (wrong base) | Med | High | Mirror kernel's feature→base map; log base at Init |

## Security Considerations
- SDHCI MMIO scoped to the SD controller; DMA (if used) authorized to its BDF. Filesystem corruption possible by a buggy Cell, but no cross-Cell escape (LBI).

## Next Steps
- Phase 08 deletes `mmc.rs` + `mmc/*` and the `MmcBlock` fallback in `block.rs`.
