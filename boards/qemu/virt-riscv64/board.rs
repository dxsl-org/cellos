use crate::{
    Architecture, BoardDescriptor, BootContract, BootProtocol, DriverId, FirmwareInterface,
    MemoryRange, MemoryRangeKind, SocId, WiringLayout,
};

const BOARD_COMPATIBLES: [&str; 2] = ["riscv-virtio", "qemu,virt"];
const ENABLED_DRIVERS: [DriverId; 5] = [
    DriverId::UartNs16550a,
    DriverId::PlicSifive,
    DriverId::ClintSifive,
    DriverId::RtcGoldfish,
    DriverId::VirtioMmio,
];
const EMPTY_WIRING: WiringLayout = WiringLayout {
    pinmux_groups: &[],
    phy_links: &[],
};
const FALLBACK_MEMORY: [MemoryRange; 3] = [
    MemoryRange {
        name: "opensbi",
        base: 0x8000_0000,
        size: 0x0020_0000,
        kind: MemoryRangeKind::Bootloader,
    },
    MemoryRange {
        name: "kernel",
        base: 0x8020_0000,
        size: 0x0400_0000,
        kind: MemoryRangeKind::Kernel,
    },
    MemoryRange {
        name: "usable",
        base: 0x8420_0000,
        size: 0x0BE0_0000,
        kind: MemoryRangeKind::Usable,
    },
];

pub const QEMU_VIRT_RISCV64: BoardDescriptor = BoardDescriptor {
    slug: "qemu-virt-riscv64",
    vendor: "qemu",
    model: "virt-riscv64",
    architecture: Architecture::Riscv64,
    soc: SocId::GenericRiscvVirt,
    compatibles: &BOARD_COMPATIBLES,
    boot: BootContract {
        firmware: FirmwareInterface::OpenSbi,
        boot_protocol: BootProtocol::DeviceTreeWithFallbackMap,
        requires_firmware_dtb: false,
        fallback_dts_path: "boards/qemu/virt-riscv64/qemu-virt-riscv64.dts",
        kernel_load_base: 0x8020_0000,
    },
    fallback_memory: &FALLBACK_MEMORY,
    wiring: EMPTY_WIRING,
    enabled_drivers: &ENABLED_DRIVERS,
};
