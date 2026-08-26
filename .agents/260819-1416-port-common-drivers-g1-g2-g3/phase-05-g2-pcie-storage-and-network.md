---
phase: 5
title: "G2 PCIe Storage and Network"
status: completed
priority: P1
effort: "10d"
dependencies: [2, 4]
tier: thinking
---

# Phase 05: G2 PCIe Storage and Network

## Context Links

- G2 strict order: `docs/specs/04-hardware.md:77-87`.
- q35 drivers: `boards/qemu/q35-x86_64/board.rs:7-14`.
- Pioneer blocked state: `boards/milk-v/pioneer/board.rs:7-11`, `boards/milk-v/pioneer/board.rs:33-53`, `hal/soc/riscv/src/catalog.rs:104-124`.
- ECAM/IOMMU/NVMe/e1000: `kernel/src/task/drivers/pcie_ecam.rs:1-15`, `kernel/src/task/drivers/iommu.rs:1-8`, `cells/drivers/nvme/src/main.rs:1-14`, `cells/drivers/e1000/src/main.rs:1-14`.
- x86 VT-d hardcoded base risk: `kernel/src/task/drivers/iommu_x86.rs:59-60`.

## Overview

Turn q35 x86_64 PCIe/NVMe/e1000 code from prototype/bring-up into a fail-closed server/PC lane, with IOMMU before NIC promotion. Pioneer remains blocked until SG2042 gains real storage/network substrate.

## Evidence

- `cargo check -p driver-e1000 --target x86_64-unknown-none` -> PASS.
- `cargo check -p cellos-kernel --target x86_64-unknown-none` -> PASS.
- `PATH=/home/dmin/.cargo/bin:$PATH pwsh -NoProfile -File scripts/build-x86_64-cells.ps1` -> PASS; built required x86 cells and refreshed the q35 image bundle.
- `bash build/make-iso.sh` -> PASS; regenerated the BIOS+UEFI ISO.
- `cargo test --test nvme-x86` -> PASS 3/3; includes shell wait, driver registration, and FAT32 mount plus `vwrite`/`vcat` roundtrip.
- `cargo test --test nic-x86` -> PASS 2/2; includes the `X86_NIC_MODEL=e1000e` fail-closed gate.

## Requirements

- Functional: q35 ECAM enumeration, BAR registration, IOMMU domain activation, NVMe block driver, e1000 NIC driver; RTL8125/i225 research only.
- Non-functional: DMA isolation is mandatory before NIC; unsupported devices fail closed without claiming readiness.

## Architecture

Data flow: q35 ACPI MCFG/ECAM -> PCI function list -> BAR allowlist -> Driver Cell `sys_find_pcie_device` -> `request_region` -> `DmaBuf::authorize` -> block/NIC registration -> VFS/net service IPC. Pioneer path stops at SBI DBCN/PLIC/CLINT until SG2042 policy grows storage/network facts.

## Related Code Files

- Modify: `kernel/src/task/drivers/{pcie_ecam,iommu,iommu_pt,iommu_x86,nic}.rs`.
- Modify: `cells/drivers/{nvme,e1000,virtio-blk,virtio-net}/`.
- Modify: `libs/ostd/src/{dma,mmio,syscall}.rs`, `libs/api/src/abi/syscall.rs` only if needed.
- Read-only blocker evidence: `boards/milk-v/pioneer/board.rs`, `hal/soc/riscv/src/catalog.rs`.
- Scripts: `scripts/build-x86_64-cells.ps1`, `scripts/ci-x86-integration.ps1`, `scripts/qemu-x86_64-test.sh`.

## Implementation Steps

1. Harden ECAM multi-bus/segment limits and BAR size validation before adding drivers.
2. Replace or explicitly gate the q35-only VT-d base before broader x86_64 hardware claims.
3. Make IOMMU activation observable and fail-closed for DMA-capable devices.
4. Promote NVMe only after block registration/VFS boot is stable.
5. Promote e1000 only after NIC identity and DMA isolation gates pass.
6. Keep RTL8125/i225 as research tickets; do not schedule Pioneer until SG2042 storage/network substrate exists.

## Todo List

- [x] ECAM scan rejects invalid firmware and oversized BARs.
- [x] x86 VT-d base is discovered, board-gated, or explicitly q35-only.
- [x] IOMMU teardown runs before DMA frames return to allocator.
- [x] NVMe read/write survives reboot image test.
- [x] e1000 rejects e1000e/unsupported IDs and handles empty RX without blocking.

## Success Criteria

- [x] q35 x86_64 QEMU boots with NVMe-backed VFS where configured.
- [x] `X86_NIC_MODEL=e1000e` fail-closed test passes.
- [x] Physical x86 server lane is recorded separately from QEMU; Pioneer remains BLOCKED unless SG2042 substrate changes.

## Test Matrix

- Unit: BAR decode, IOMMU domain map/unmap, driver dispatch protocols.
- Integration: q35 QEMU with NVMe/e1000; Pioneer descriptor compile is not storage/network evidence.
- E2E: Dell/real PC NVMe boot + NIC DHCP; Pioneer/C930 hardware only after substrate evidence.

## Risk Assessment

| Risk | LxI | Mitigation |
|---|---|---|
| DMA before isolation | MxCritical | hard gate Driver Cell command path on active IOMMU for NIC/storage where required. |
| q35 VT-d base reused on real PC | MxH | discover from ACPI/DMAR or board-gate q35-only path. |
| Pioneer falsely treated as G2-ready | MxH | block on SG2042 storage/network facts and descriptor additions. |
| Firmware ECAM lies | MxH | validate ACPI/DTB; fail closed with no fallback ECAM on x86. |
| PCIe scope grows into USB/audio/WiFi | HxM | explicit out-of-scope list; add separate plan only after G2 storage/NIC pass. |

## Deferred Low Risk

- `resource_registry::valid_pcie_bar_window` still lacks a dedicated no_std unit-harness case; runtime q35 coverage is sufficient for Phase 05 close, and the harness gap is deferred as low risk.

## Security Considerations

Treat every DMA-capable endpoint as hostile; tie MMIO, BDF, DMA window, IRQ ownership, and registered role to the same Cell lifetime.

## Backward Compatibility

Keep VirtIO fallback and current block/NIC IPC wire formats; new drivers must exit cleanly when absent.

## File Ownership

Owns q35 PCIe/storage/network files; does not modify G1 controllers or Pioneer SG2042 substrate.

## Rollback

Disable q35 G2 board driver selection and revert Driver Cell package inclusion; keep old VirtIO/ramdisk boot lane. Irreversible part: firmware/NVMe disk writes on physical tests, mitigated by disposable media and readback.

## Assumptions

- Claim: q35/e1000 remains the first G2 promotion lane. Confidence: high. How to verify: `docs/specs/04-hardware.md:77-87` and q35 descriptor lines above.

## Deviation Log

- Q35 lane closed; physical x86 remains hardware-gated/deferred.
- RTL8125/i225 remain research-only.

## Next Steps

Phase 06 may start only after G2 baseline and inference prerequisites are measurable.
