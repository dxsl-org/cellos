# aarch64-vm — Generic AArch64 Virtual Machine Profile

## Purpose

The `aarch64-vm` board descriptor represents an AArch64 virtual machine hosted under hypervisors on Apple Silicon macOS (Virtualization.framework / vfkit / QEMU with `-accel hvf`) or Linux KVM.

## Architecture & Hardware Contracts

- **Architecture:** AArch64 (ARMv8-A / ARMv8.5+ with VHE).
- **Interrupt Controller:** GICv3 (Distributor, Redistributors, ICC System Registers).
- **Secondary Cores:** PSCI `CPU_ON` over HVC conduit.
- **Console:** PL011 UART via MMIO.
- **PCIe & I/O:** Generic ECAM host controller (`pci-host-ecam-generic`) supporting VirtIO devices over PCI.
- **DMA Isolation:** `Optional` (governed by the host hypervisor).
