---
phase: 18
title: "Driver model and Tier-1 TCB perimeter"
status: in-progress
priority: P2
effort: "8–12d"
dependencies: [5, 12]
tier: medium
checkpoint: false
---

# Phase 18: Driver model and Tier-1 TCB perimeter

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs.

## Context Links

[Review H-12, Medium hardware](../reports/architecture-review-261110.md) · `cells/drivers/{igc,ixgbe,i40e,r8169,bcm57xx,e1000,e1000e,igb,vmxnet3}/` · `libs/virtio-pci-transport/` · `kernel/src/task/drivers/{driver_cell,iommu}.rs` · `kernel/src/resource_registry.rs` · `scripts/cellos_sign/__init__.py` · `scripts/unsafe-allowlist.toml` · `kernel/src/loader/governed_spawn.rs` · [Spec 13](../../docs/specs/13-peripherals.md) · [Spec 16](../../docs/specs/16-rustc-tcb.md) · [ADR-0015](../../docs/decisions/0015-dual-mode-hybrid-architecture.md)

## Overview

Stop multiplying driver scaffolding and device-ownership rules, and turn the Tier-1 language-isolation perimeter from a convention into a checked property of the signing pipeline.

## Key Insights

- Reviewer-cited: five NIC cells differ by 6–38 lines in `dispatch.rs`/`main.rs`; `dma_layout.rs` is copied with its own unsafe `try_init_array`; the sender-authentication gate is duplicated per cell; NIC role is a last-writer-wins singleton (`driver_cell.rs:249-261`) while block uses per-device routes; MMIO, IRQ and DMA ownership are decided by three unrelated rules; the DMA remapper interface carries VT-d-only fields and a second, empty `ArmIommu` design exists.
- Reviewer-cited: the unsafe scan covers `cells/` only; `libs/ostd` (231 `unsafe`) and `libs/api` (164) link into every cell; Spec 16's TCB table omits `ostd`; the tier is chosen by the cell's own manifest class (`governed_spawn.rs:68-70`) and nothing ties a `c-ffi` allowlist class to an FFI/UNTRUSTED manifest.

## Requirements

- Functional: `libs/nic-driver` holding bring-up, dispatch, sender authentication and DMA ring layout, with a small `NicDevice` trait per chip; NIC routes keyed by BDF like block routes; one per-device ownership record (owner incarnation, BARs, IRQs, DMA domain) consulted by MMIO, IRQ and DMA checks; one kernel `DmaRemapper` interface with VT-d as the only implementation and `ArmIommu` removed until a backend exists; signer refuses a crate whose allowlist class is `c-ffi` (or `legacy-raw-ptr`) unless its manifest class is FFI/UNTRUSTED; the unsafe scan covers the dependency closure linked into Tier-1 cells, with `libs/*` entries listed and ratcheted; Spec 16 TCB table lists the real closure.
- Non-functional: NIC wire protocol unchanged (proto module from Phase 12); no driver gains authority.

## Architecture

```
libs/nic-driver ──► cells/drivers/<chip>: impl NicDevice { probe, init, tx, rx, mac, irq }
kernel dev/ownership.rs:  DeviceRecord { key: Bdf | MmioBase, owner, bars, irqs, dma_domain }
        ◄── RequestMmio, WaitIrq/MSI, map_dma_for_cell, teardown
signer:  allowlist class × manifest class matrix ─► refuse on mismatch
```

## Assumptions

- **Claim:** the nine NIC cells share enough structure for one trait without chip-specific escape hatches in the library.
  **Confidence:** medium
  **How to verify:** diff `main.rs`/`dispatch.rs`/`dma_layout.rs` across all nine before fixing the trait.
- **Claim:** existing Tier-1 cells with `c-ffi` allowlist entries (reviewer counted 41) can either move to Tier 2 on x86 after Phase 15 or are already FFI-class.
  **Confidence:** low
  **How to verify:** join `scripts/unsafe-allowlist.toml` classes with each crate's `declare_manifest!` class; the mismatch list is the work list and may need owner triage (some may have to stay Tier 1 with a documented exception).
- **Claim:** scanning `libs/*` will produce a large initial list that must be accepted as baseline.
  **Confidence:** high
  **How to verify:** run the scanner on `libs/ostd`.

## Related Code Files

- Create: `libs/nic-driver/`, `kernel/src/dev/ownership.rs` (post-Phase-09 path), `scripts/check-tier-class.py`
- Modify: the nine NIC cells, `kernel/src/task/drivers/{driver_cell,iommu,iommu_x86*}.rs`, `kernel/src/resource_registry.rs`, MMIO/IRQ/DMA syscall arms, `hal/arch/arm/src/aarch64/backend/mod.rs`, `scripts/cellos_sign/__init__.py`, `scripts/unsafe-allowlist.toml`, `docs/specs/{13-peripherals,16-rustc-tcb}.md`, root `Cargo.toml`
- Delete: per-cell copies of `dma_layout.rs`/`dispatch.rs`, `ArmIommu`, aarch64 snapshot MMIO tables once `hal/soc` is the single source

## Implementation Steps

1. Diff the NIC cells; define `NicDevice`; extract the library with two cells (igc, r8169), then the rest.
2. NIC routes by BDF; fixture with two NICs in QEMU proves both stay bound.
3. `DeviceRecord`; switch MMIO, IRQ and DMA checks to it one at a time; teardown releases through it.
4. `DmaRemapper` interface; move VT-d behind it; delete the empty ARM design.
5. Join allowlist classes with manifest classes; write `check-tier-class.py`; wire into the signer; triage mismatches with the owner.
6. Extend the unsafe scan to the Tier-1 dependency closure; baseline and ratchet; correct Spec 16 and Spec 13.

## Todo List

- [ ] `libs/nic-driver`, cells migrated
- [ ] Per-BDF NIC routes
- [ ] Device ownership record
- [ ] `DmaRemapper`
- [ ] Tier-class gate in signer
- [ ] TCB closure scanned

## Success Criteria

- [ ] x86 QEMU with two NICs of the same and of different models: both registered, traffic on each; restarting one driver does not unbind the other.
- [ ] Signing a crate with a `c-ffi` allowlist entry and a Tier-1 manifest fails (negative control fixture).
- [ ] Unsafe scan reports `libs/ostd` and `libs/api` entries and fails on a new unlisted `unsafe` block there.
- [ ] NIC lanes (e1000, e1000e, igb, vmxnet3, virtio-net) pass on x86; `x86-c2c` passes.

## Risk Assessment

- Physical-only NICs (igc, ixgbe, i40e, r8169, bcm57xx) cannot be runtime-verified in QEMU: host tests only; state that ceiling in evidence. Rollback: per-cell revert.
- The tier gate may show Tier-1 cells that cannot move yet: record explicit, signed exceptions rather than weakening the gate. Already-signed artifacts are unaffected.

## Security Considerations

A memory bug in an unscanned shared library defeats language isolation for every Tier-1 cell; the scan and the tier gate are the structural controls. Per-device ownership removes spawn-order as an authority input.

## Next Steps

Phase 19 closure.

## Deviation Log

- **Decision (2026-11-16, owner, census defaults).** The census is in [research/p18-census-drivers.md](research/p18-census-drivers.md) and [research/p18-census-tcb.md](research/p18-census-tcb.md).
  - The two-NIC criterion is narrowed: both NICs register on routes keyed by their own device, the primary carries traffic, and restarting one leaves the other bound. Multi-interface networking (a read-only route-list syscall and a multi-interface net service) is a residual; the ABI is unchanged.
  - `libs/boot-config` accepts two cells with the same `path`, so the fixture covers two NICs of the same model.
  - The three authority holes close through `DeviceRecord`:
    - only the device owner may register a NIC route;
    - `RequestMmio` grants a PCIe BAR only to the device owner;
    - `WaitIrq` checks IRQ ownership on every architecture.
  - The device owner stays the claiming thread.
  - TCB scan:
    - third-party crates are pinned at `name@version`;
    - the baseline is the combined total of cells plus the closure;
    - `#[no_mangle]`/`#[export_name]`/`#[link_section]` are counted as a second ceiling per file;
    - `app-c-math-smoke` declares `PROTECTION_CLASS_FFI`.
  - `service-net` and `service-compositor` become signed tier exceptions.
- **Deviation.** The criterion "unsafe scan reports `libs/ostd` and `libs/api`" names a crate removed in `077909a812`. It is read as "`libs/ostd` and the other in-tree crates of the Tier-1 closure".
- **Deviation.** `vmxnet3` keeps its own DMA area and its 9 `unsafe` lines; `libs/nic-driver` holds the shared ring layout used by the other eight.
- **Surprise.** The five physical-only NICs (igc, ixgbe, i40e, r8169, bcm57xx) cannot bind on any machine today: no catalog grant, no `services.toml` entry, no kernel Ethernet ID row, and empty device ID lists. Binding them is outside Phase 18 and is recorded as a residual for the S5 captures.
- **Surprise.** A 610-line RISC-V IOMMU backend shares the remapper interface with VT-d, so VT-d is not the only implementation. Both go behind `DmaRemapper`. The kernel's `AppleDartPt`, used only by its own tests, is a second empty ARM design and is removed with `ArmIommu`.
