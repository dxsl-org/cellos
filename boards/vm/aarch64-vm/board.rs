// SPDX-License-Identifier: MPL-2.0
use crate::{
    Architecture, BoardDescriptor, BootContract, BootProtocol, DmaIsolation, DriverId,
    FirmwareInterface, SocId, WiringLayout,
};

const COMPATIBLES: [&str; 2] = ["cellos,aarch64-vm", "linux,dummy-virt"];

const DRIVERS: [DriverId; 7] = [
    DriverId::GicV3,
    DriverId::UartPl011,
    DriverId::PcieEcam,
    DriverId::VirtioPci,
    DriverId::VirtioMmio,
    DriverId::RtcPl031,
    DriverId::SdhciArasan,
];

/// Generic AArch64 **virtual machine** descriptor (Phase 03, ADR-0032 §5).
///
/// Designed for hosted guests on Apple Silicon macOS (Virtualization.framework / vfkit / QEMU hvf).
/// Features GICv3 interrupt handling, PSCI SMP boot, PCIe ECAM, and VirtIO devices over PCI.
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
        requires_firmware_dtb: true,
        fallback_dts_path: "",
        kernel_load_base: 0x4008_0000,
    },
    fallback_memory: &[],
    wiring: WiringLayout {
        pinmux_groups: &[],
        phy_links: &["pl011-uart", "pcie-root"],
    },
    enabled_drivers: &DRIVERS,
    dma_isolation: DmaIsolation::Optional,
};
