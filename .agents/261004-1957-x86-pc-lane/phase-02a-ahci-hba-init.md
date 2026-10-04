---
phase: 2
sub: a
title: "AHCI part A — PCI binding, HBA init, IDENTIFY"
status: completed
priority: P1
effort: M (sub-phase of the AHCI family)
dependencies: [1]
tier: thinking
ceiling: qemu
---

# Phase 02a — AHCI part A: PCI binding, HBA init, IDENTIFY

## Evidence (2026-10-04/05, `qemu` ceiling)

- New cell `cells/drivers/ahci/` (759 lines: `main.rs`, `controller.rs`, `dma.rs`,
  `build.rs`) — PCI binding on `0x01/0x06/0x01`, ABAR via `PcieDeviceInfo::bar_mem_base`,
  HBA reset, port selection, one polled IDENTIFY DEVICE. No READ/WRITE, no block
  registration (`main.rs` documents the part-A/part-B boundary).
- With a 64 MiB raw image on q35's ICH9 AHCI, independently re-verified by me after
  the implementer's run: `[ahci] ABAR claim ok base=0xfebd5000 len=0x1000`,
  `controller bound bdf=00:1f.2 cap=0xc0141f05 version=0x00010000 pi=0x0000003f`,
  `HBA reset complete (AE enabled)`, `port 2 is not an ATA disk (SSTS=0x113
  SIG=0xeb140101); skipping` (the ATAPI CD-ROM), `port 5 link up`, `IDENTIFY DEVICE
  ok port=5 sectors=131072 model="QEMU HARDDISK" fw="2.5+"`,
  `[driver_cell] ahci storage driver ready`, then `Cellos >`.
- Diskless boot (the CI shape): shell reached, `[ahci] no SATA disk attached;
  driver cell idle` — the cell is inert on a machine with no disk instead of
  erroring.
- `cargo test --test ahci-x86` 2/2 (not a skip); `cargo test -p cellos-boards`
  13/13 with the `StorageAhci` assertion flipped; `check-hal-boundaries.sh` pass;
  kernel `cargo check` (q35 default) clean.
- Re-verified after the review fixes (same day): the `board-x86-pc` ISO still
  boots to `Cellos >` with the identical marker set (`ABAR claim ok` → … →
  `IDENTIFY DEVICE ok` → `storage driver ready`), the diskless boot still logs
  `no SATA disk attached; driver cell idle`, and `ahci-x86` is 2/2 with no leaked
  temp image (`/tmp/vicell_sata_x86_*` = 0 after the run).
- Evidence log: `evidence/phase-02a-ahci-identify.log`.

## Interface changes this phase had to make (recorded, not incidental)

1. **ABI append**: `PcieDeviceInfo` (24 → 40 bytes) gained `bar_mem_base` /
   `bar_mem_len` (first *memory* BAR). `bar0_base`/`bar0_len` keep their exact
   meaning; ICH9's ABAR is BAR5, so a SATA driver needs the appended fields. Safe
   only because every cell is rebuilt by the same packaging pass — there is no
   stale 24-byte reader in-tree; the `ostd` doc comment states this.
2. **Boot wiring**: `/bin/ahci` joined the launch-profile target list, the
   boot_ceiling PCIe row, `with_path_caps`, the init `start_block_drivers()` spawn
   (one surgical hunk; the file's other hunks are the user's WIP), the dev policy
   and the CI sign/fat32 lists.
3. **Two boot-time races found and fixed during bring-up**: the Platform Cell's
   BAR-size probe temporarily clears memory decode (the cell now waits for a sane
   CAP/PI before touching ABAR), and `PxSIG` is only valid after the port is
   started (QEMU returns `0xffffffff` before), so port selection starts the engine
   before reading the signature.
4. **Unsafe policy corrected in the plan**: Driver Cells use the documented Law-4
   exception (as NVMe/e1000 do), not `#![forbid(unsafe_code)]`; the F1 unsafe
   ratchet stays green.

## Review fixes (2026-10-05, independent reviewer)

The reviewer confirmed part-A scope and the part-A/part-B boundary, and raised
gaps that q35 tolerates but real hardware need not. Fixed before shipping:

1. `GHC.AE` is enabled before any register other than `GHC` is touched, and
   `GHC.HR` is now requested with AE held (writing HR alone cleared AHCI mode) —
   AHCI 1.3.1 §10.1.2, which only matters on a controller whose `CAP.SAM` is 0.
2. Port bring-up waits for the initial D2H FIS: FIS receive on → `PxSERR`
   cleared → `PxTFD` BSY/DRQ idle (bounded) → `PxSIG` polled for a *known*
   signature, and only then `PxCMD.ST`. QEMU's ICH9 model publishes `PxSIG` only
   after ST, so the code falls back to the started-engine read and the existing
   markers are unchanged.
3. IDENTIFY validates the transfer before emitting either success marker
   (command-header PRDBC == 512 and a non-degenerate word 0), so a
   completed-but-empty DMA cannot be reported as a good disk.
4. COMRESET is held for ≥1 ms against the monotonic clock
   (`ostd::syscall::sys_get_time_ms`), not an iteration count whose duration
   depends on CPU frequency and scheduler state.
5. The temp SATA image is owned by a `Drop` guard, so a panic inside the runner
   constructor cannot leak a 64 MiB file.
6. The test requires the ISO to actually carry `/bin/ahci` (the kernel's
   launch-path literal) instead of passing on a stale shared ISO.
7. The `bar_mem_*` dependency on the kernel's own ECAM scan is recorded as a plan
   risk: the Platform-Cell registration path stores only BAR0, so extending it
   belongs to that owner; the cell fails closed with a named error meanwhile.

## Target

Roadmap gate **X86-PC-1**, first half. `grep -ri ahci` over the Cellos source
(`kernel/src/task`, `cells/drivers`, `hal/soc`, `boards`) has no hits; matches
exist only inside the embedded guest Linux artifacts. Without a storage driver an
industrial SATA-only PC has no persistent `/data` and no persistent guest disk.

## Change

- New Driver Cell `cells/drivers/ahci/` on the `nvme`/`e1000` pattern (Tier 1
  driver cell): `request_mmio` BAR handoff, every DMA buffer through
  `DmaBuf::authorize` so phase 05's IOMMU work applies without rework.
- Unsafe policy: Driver Cells use the documented **Law-4 exception** (as the
  NVMe/e1000 cells do) — every `unsafe` block carries a `// SAFETY:` comment,
  MMIO goes through the bounds-checked `MmioRegion`, and the F1 unsafe ratchet
  must stay green.
- PCI binding: class `0x01`/`0x06`, prog-if `0x01` (AHCI). Any other prog-if
  (RST/RAID-only firmware mode) **fails closed** with a log line naming the
  prog-if — this is what gives the HCL "SATA in AHCI mode" row teeth.
- AHCI 1.3.1 part A scope: HBA reset, global/port enable, command list +
  FIS receive area allocation, port link spin-up detection, IDENTIFY DEVICE, and
  a bounded polled completion path. No data transfer in this sub-phase.
- Descriptor wiring (verified pre-Build): `DriverId` variant for AHCI storage in
  `boards/src/descriptor.rs` + a `boards/src/catalog_tests.rs` row. Phase 01
  declares the variant so this phase owns disjoint files.
- Packaging: extend `scripts/build-x86_64-cells.ps1` to build the cell and assert
  it is present in the FAT image (pattern from the e1000 DHCP gate,
  `.agents/260903-x86-e1000-dhcp/plan.md`).

## QEMU-first gate

- New `QemuRunner` constructor `boot_x86_bios_with_sata(iso, sata_disk)`:
  q35's built-in ICH9 AHCI with a raw image
  (`-drive file=<img>,if=none,id=sata0,format=raw -device ide-hd,drive=sata0`).
  Verified in the premise smoke: the controller enumerates as `8086:2922`
  class `01:06:01`.
- New `tests/integration/tests/ahci-x86.rs` (modelled on `nvme-x86.rs` and
  `nic-x86.rs`): assert the cell binds the controller, the HBA/port come up, and
  IDENTIFY reports a device. Add the SATA options to
  `scripts/qemu-x86_64-test.sh` so the lane is runnable without the Rust harness.
- Record the QEMU-model caveat: ICH9 AHCI is register-compatible; port count,
  remap, and NCQ behaviour are not validated here (`A-02`).

## Acceptance

- On QEMU: the driver cell registers the controller and logs IDENTIFY success;
  `x86_64-boot` stays 7/7 and `nvme-x86` stays 3/3.
- A SATA controller in a non-AHCI prog-if fails closed with a named reason.
- No new unsafe island; the unsafe ratchet stays satisfied.

## Out of scope

- Data transfer, block registration, filesystem (that is 02b).
- NVMe (shipped), port multipliers, hotplug, TRIM/NCQ tuning, RAID, SAS/HBA.

## Risk assessment

- **Undo:** delete the cell + its packaging entry + the test; the embedded VIFS1
  boot path is untouched, so the system returns to today's behaviour.
- **Not undoable:** any claim that a physical machine's storage works — phase 07.
