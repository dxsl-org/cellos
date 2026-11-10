// SPDX-License-Identifier: MPL-2.0
use crate::{
    Architecture, BoardDescriptor, BootContract, BootProtocol, DmaIsolation, DriverId,
    FirmwareInterface, MemoryRange, MemoryRangeKind, SocId, WiringLayout,
};

const COMPATIBLES: [&str; 2] = ["cellos,aarch64-vm", "linux,dummy-virt"];

const DRIVERS: [DriverId; 6] = [
    DriverId::GicV3,
    DriverId::UartPl011,
    DriverId::PcieEcam,
    DriverId::VirtioPci,
    DriverId::VirtioMmio,
    DriverId::RtcPl031,
];

/// QEMU virt RAM starts at 0x4000_0000; the kernel loads at +0x8_0000.
pub const FALLBACK_KERNEL: MemoryRange = MemoryRange {
    name: "kernel",
    base: 0x4000_0000,
    size: 0x0200_0000,
    kind: MemoryRangeKind::Kernel,
};
/// Usable RAM up to 256 MiB when the firmware tree gives none; a tree memory
/// node that covers the RAM base replaces this ceiling.
pub const FALLBACK_USABLE: MemoryRange = MemoryRange {
    name: "usable",
    base: 0x4200_0000,
    size: 0x0E00_0000,
    kind: MemoryRangeKind::Usable,
};
const MEMORY: [MemoryRange; 2] = [FALLBACK_KERNEL, FALLBACK_USABLE];

/// Generic AArch64 **virtual machine** descriptor (Phase 03, ADR-0032 §5).
///
/// Bound by the kernel's `board-aarch64-vm` feature (HAL backend
/// `aarch64_vm`): the QEMU `virt` memory map with a GICv3, under QEMU TCG,
/// Apple HVF or KVM. GICv3 distributor/redistributors and RAM come from the
/// firmware tree when one is passed (`-kernel Image`), otherwise from QEMU
/// virt's fixed map. PSCI secondaries, PL011 console.
/// `dma_isolation` is `Optional`: the hypervisor owns host-virtual translation.
pub const AARCH64_VM: BoardDescriptor = BoardDescriptor {
    slug: "aarch64-vm",
    vendor: "generic",
    model: "aarch64-vm",
    architecture: Architecture::Aarch64,
    soc: SocId::GenericAArch64Vm,
    compatibles: &COMPATIBLES,
    boot: BootContract {
        firmware: FirmwareInterface::Uefi,
        boot_protocol: BootProtocol::DeviceTreeWithFallbackMap,
        // A direct ELF boot passes no tree; the QEMU virt map then applies.
        requires_firmware_dtb: false,
        fallback_dts_path: "boards/vm/aarch64-vm/aarch64-vm.dts",
        kernel_load_base: 0x4008_0000,
    },
    fallback_memory: &MEMORY,
    wiring: WiringLayout {
        pinmux_groups: &[],
        phy_links: &["pl011-uart", "pcie-root"],
    },
    enabled_drivers: &DRIVERS,
    dma_isolation: DmaIsolation::Optional,
};
