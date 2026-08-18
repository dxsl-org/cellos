use crate::{
    Architecture, BoardDescriptor, BootContract, BootProtocol, DriverId, FirmwareInterface,
    MemoryRange, MemoryRangeKind, SocId, WiringLayout,
};

const COMPATIBLES: [&str; 1] = ["linux,dummy-virt"];
const DRIVERS: [DriverId; 5] = [
    DriverId::UartPl011,
    DriverId::GicV2,
    DriverId::RtcPl031,
    DriverId::VirtioMmio,
    DriverId::PcieEcam,
];
pub const FALLBACK_KERNEL: MemoryRange = MemoryRange {
    name: "kernel",
    base: 0x4000_0000,
    size: 0x0200_0000,
    kind: MemoryRangeKind::Kernel,
};
pub const FALLBACK_USABLE: MemoryRange = MemoryRange {
    name: "usable",
    base: 0x4200_0000,
    size: 0x0E00_0000,
    kind: MemoryRangeKind::Usable,
};
const MEMORY: [MemoryRange; 2] = [FALLBACK_KERNEL, FALLBACK_USABLE];
pub const QEMU_VIRT_AARCH64: BoardDescriptor = BoardDescriptor {
    slug: "qemu-virt-aarch64",
    vendor: "qemu",
    model: "virt-aarch64",
    architecture: Architecture::Aarch64,
    soc: SocId::QemuArmVirt,
    compatibles: &COMPATIBLES,
    boot: BootContract {
        firmware: FirmwareInterface::Uefi,
        boot_protocol: BootProtocol::DeviceTreeWithFallbackMap,
        requires_firmware_dtb: false,
        fallback_dts_path: "boards/qemu/virt-aarch64/qemu-virt-aarch64.dts",
        kernel_load_base: 0x4008_0000,
    },
    fallback_memory: &MEMORY,
    wiring: WiringLayout {
        pinmux_groups: &[],
        phy_links: &[],
    },
    enabled_drivers: &DRIVERS,
};
