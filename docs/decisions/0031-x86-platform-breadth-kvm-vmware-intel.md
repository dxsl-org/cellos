# ADR-0031 — x86-64 platform breadth: KVM, VMware and Intel PC/server

**Date:** 2026-11-04
**Status:** Proposed — awaiting owner approval. It amends [ADR-0022](0022-intel-x86-64-c2c-only-direction.md) and depends on [ADR-0029](0029-platform-image-families-and-runtime-detection.md).
**Decider:** Cellos maintainer (owner), through the planning conversation of 2026-11-03/04.
**Implementation authority:** This ADR changes direction and scope only. Every ABI change still needs Law-1 CP1 and CP2 (`docs/code-standards.md` Law 1). Every file outside an existing handoff still needs a CELL-CONTRACT-0024 lock handoff. No hardware purchase is approved.
**Plan:** [`.agents/261103-2218-x86-platform-breadth/plan.md`](../../.agents/261103-2218-x86-platform-breadth/plan.md)

## Context

ADR-0022 §Hardware item 2 qualifies "one exact Intel machine configuration" and says "Keep one NIC/storage path; do not broaden device support for convenience". Two facts limit that path:

1. **Only QEMU runs Cellos x86 today.** Default KVM guests (libvirt/Proxmox/OpenStack: virtio devices, often no HPET), VMware guests, and most current Intel PCs do not boot past these closed gates:
   - Timer: HPET is mandatory (`kernel/src/main.rs:512`).
   - APIC: only xAPIC is supported (`hal/arch/x86/src/x86_64/apic.rs:22,233`).
   - CPUs: at most 2 (`kernel/src/task/smp.rs:16`).
   - Devices: no virtio-pci transport on x86 (`cells/drivers/virtio-blk/src/device.rs:175-178`); all PCIe drivers poll, with no MSI/MSI-X.
   - Console: COM1 only.
   - NICs: e1000 82540EM and igb 82576/i210 only.
2. **Two-node C2C needs cheap, repeatable nodes before and alongside physical hardware.** KVM and VMware guests on the developer host provide them. Physical qualification gets more machine choices once the common Intel PCH NICs and platform features work.

The scouting, validation and red-team evidence is in the plan's `scout-report.md`, `reports/validation-log.md` and `reports/red-team.md`.

## Decision

1. **Targets.** One x86_64 image, through the ADR-0029 catalog, targets:
   - KVM (q35, default virtio devices, HPET optional);
   - VMware virtual hardware (e1000/e1000e/vmxnet3, AHCI/NVMe/PVSCSI);
   - Intel physical PCs and servers.
   ADR-0022's "keep one NIC/storage path" sentence is amended to allow the driver families listed in the plan (phases 05, 08, 09, 13-19), each admitted by exact device ID only.
2. **Out of scope:**
   - AMD (stays parked; no AMD-Vi);
   - cloud-specific devices (ENA, gVNIC, Hyper-V VMBus);
   - GPU drivers (in this plan; native Intel iGPU compute has its own plan under [ADR-0033](0033-x86-intel-igpu-native-compute.md), 2026-11-04);
   - mlx5/bnxt/ice;
   - an Intel VMD driver;
   - MegaRAID/mpt3sas/smartpqi.
3. **VMD and RAID are HCL constraints.** A qualified machine has VMD disabled. Its storage is NVMe/AHCI, or a RAID controller in HBA mode exposing NVMe/AHCI (HCL R4).
4. **D-VM: DMA policy in guests (owner decision 2026-11-03).**
   - `x86_64-pc` stays `DmaIsolation::Required`.
   - A new `x86_64-vm` catalog profile carries `DmaIsolation::Optional`. Under ADR-0029 §2 it binds only on an explicit `cellos.profile=x86_64-vm` with the CPUID hypervisor bit set; this is already enforced for every non-`Required` profile at `kernel/src/board.rs:279`.
   - Production guests run `x86_64-pc` with a vIOMMU: KVM `intel-iommu` with virtio `iommu_platform=on`, or VMware "Virtualize IOMMU".
   - Under `Required`, virtio drivers must negotiate `VIRTIO_F_ACCESS_PLATFORM` or refuse the device.
5. **Interrupt remapping becomes part of `Required`** once the plan's phase 12 ships. A physical machine without DMAR `INTR_REMAP` fails HCL R8. Without IR, a Cell can make its device DMA into the MSI window and spoof interrupts (plan red team finding 1).
6. **HCL relaxations take effect only when their mechanism ships:**
   - R2 "HPET" becomes "reliable clocksource: invariant TSC, kvmclock, HPET or ACPI PM timer" (phase 02).
   - R1 "COM1" becomes "console via SPCR/COM, BMC SOL, or GOP framebuffer + PS/2/USB keyboard" (phase 10).
7. **Physical-only NIC drivers are not done without a capture.** A NIC family with no QEMU model is complete only after a physical capture on a machine carrying that NIC. Until then its phase is `blocked: hardware`.
8. **The ADR-0022 admission rule still applies to every slice:** it must name its C2C link, its acceptance scenario and its evidence ceiling. The single-WIP rule is unchanged.

## Considered options

| Option | Outcome | Reason |
|---|---|---|
| Keep ADR-0022 as is (one fixed configuration) | Rejected | KVM/VMware C2C nodes and physical selection both stay blocked by platform-core gaps that have nothing to do with device breadth |
| Include AMD (AMD-Vi, Ryzen/EPYC) | Rejected for now | Adds an IOMMU program; owner kept AMD parked (2026-11-03) |
| Write VMD and RAID drivers | Rejected for now | No QEMU model for VMD; RAID firmware interfaces are large; BIOS settings achieve the same outcome |
| vIOMMU-only VMs (no `x86_64-vm`) | Rejected | Default libvirt/VMware guests would not run DMA drivers at all; the explicit, hypervisor-only profile keeps ADR-0029 §2 intact |
| Scope as recorded above | **Chosen** | Unblocks KVM/VMware nodes first (S1-S3), then the Intel physical platform (S4-S6), with exact-ID admission throughout |

## Consequences

- New Law-1 items, listed in the plan's CP1 package (`cp1-pcie-irq-power-abi.md`):
  - `QueryPcieFunction = 434`, `AllocPcieIrq = 435`, `SystemPower = 436`;
  - a "skip devices owned by a live driver" semantic for `FindPcieDevice = 418` / `FindPcieDeviceByVendor = 424`.
- `MAX_HARTS` on x86_64 grows from 2 (target 256). riscv64/aarch64 stay at 2.
- The G1 private lane (`cellos_g1_cpu`) keeps its HPET contract unchanged.
- Each new driver Cell ships in the x86 image and the `qemu-x86_64-boot` CI job in the same change.
- No production, remote or physical support claim follows from this ADR. Claims are bounded by the evidence ladder in ADR-0022.

### Amendment 2026-11-04 — GPU dependency (ADR-0033)

GPU drivers stay out of this plan. The native Intel iGPU compute plan (`.agents/261104-0801-x86-igpu-compute/`) depends on phases 04a (PCIe function ABI, MSI-X), 04b (IRQ-driven PCIe Cells) and 12 (VT-d multi-unit, RMRR, interrupt remapping). Those phases keep their scope and ownership; the GPU plan consumes them after they land.

## Related

- [ADR-0022](0022-intel-x86-64-c2c-only-direction.md), [ADR-0029](0029-platform-image-families-and-runtime-detection.md), [ADR-0024](0024-cell-kernel-authority-contract.md)
- [HCL](../hardware-compatibility-list.md), [ABI boundary records](../abi-boundary-records.md)
