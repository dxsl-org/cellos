---
phase: 3
title: "Hosted track: Cellos as a guest on Apple Silicon macOS"
status: pending
priority: P2
effort: "6d"
dependencies: ["2a", "2c"]
tier: thinking
checkpoint: false
---

# Phase 03: Hosted guest on Apple Silicon macOS (AS-3)

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Context Links

- [ADR-0032](../../docs/decisions/0032-apple-silicon-secondary-lane.md) §5, §8; [ADR-0031](../../docs/decisions/0031-x86-platform-breadth-kvm-vmware-intel.md) §4 (D-VM)
- x86 breadth [phase 05](../261103-2218-x86-platform-breadth/phase-05-virtio-pci-cells.md) (virtio-pci cells, Intel-owned)

## Overview

Run Cellos on a Mac without replacing macOS: a Virtualization.framework guest (through a small CLI such as `vfkit`, Apache-2.0) or QEMU with `-accel hvf`. This is the earliest way to put a Cellos node on a Haily user's Mac. It is virtual-hardware evidence (`S1-hvf`), not Apple SoC evidence.

## Key Insights

- Virtualization.framework guests see GICv3, PSCI over HVC, a generated FDT, PCIe ECAM and virtio-pci devices. Cellos `virt` uses virtio-mmio and GICv2 today, so this needs 02c's GICv3 and the virtio-pci cells from x86 phase 05 built for aarch64.
- ADR-0029 §2 needs a detected fact for a weaker DMA profile. On aarch64 there is no CPUID hypervisor bit; use the FDT root `compatible` of the hypervisor plus an explicit `cellos.profile=aarch64-vm`.

## Requirements

- `aarch64-vm` catalog profile (`DmaIsolation::Optional`), bound only by explicit selection plus a matching FDT fact; logged at boot and recorded in the machine pin.
- Generic FDT `pci-host-ecam-generic` enumeration on aarch64; virtio-pci net/blk cells built for aarch64.
- Runner script for macOS (`tools/apple-host/`), with exact tool versions recorded.

## Related Code Files

- Create: `boards/vm/aarch64-vm/**`, `tools/apple-host/run-vz.sh` (or `run-hvf.sh`)
- Modify: aarch64 backend selection, `kernel/src/task/drivers/pcie_ecam.rs` (FDT source), virtio-pci cell build lists, `kernel/src/board.rs`

## Implementation Steps

1. FDT-sourced ECAM enumeration on QEMU `virt` (`highmem`, PCIe) — S1-qemu.
2. virtio-pci cells for aarch64 (after x86 phase 05 lands).
3. `aarch64-vm` profile and binding rule.
4. macOS runner; boot, DHCP and persistence on an Apple host.

## Todo List

- [ ] FDT ECAM
- [ ] virtio-pci aarch64
- [ ] `aarch64-vm` profile
- [ ] macOS runner + S1-hvf capture

## Success Criteria

- [ ] On an Apple Silicon Mac (S1-hvf): shell, DHCP over virtio-net, two-boot persistence over virtio-blk, with the exact macOS, tool and Cellos image identities recorded.
- [ ] Without `cellos.profile=aarch64-vm` the same image refuses untranslated DMA.

## Risk Assessment

- Virtualization.framework behaviour changes between macOS releases; pin and record the version per capture.

## Security Considerations

Inside `aarch64-vm`, Cells are not DMA-isolated from each other (same consequence as ADR-0031 D-VM). State this in the boot log line and the board README.

## Next Steps

Phase 08 can measure inference on this track.

## Deviation Log

None.
