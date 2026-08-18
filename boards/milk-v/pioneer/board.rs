use crate::{
    Architecture, BoardDescriptor, BootContract, BootProtocol, DriverId, FirmwareInterface,
    MemoryRange, MemoryRangeKind, SocId, WiringLayout,
};

const COMPATIBLES: [&str; 2] = ["sophgo,pioneer", "sophgo,sg2042"];
const DRIVERS: [DriverId; 3] = [
    DriverId::ConsoleSbiDbcn,
    DriverId::PlicSifive,
    DriverId::ClintSifive,
];
const MEMORY: [MemoryRange; 3] = [
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

pub const MILK_V_PIONEER: BoardDescriptor = BoardDescriptor {
    slug: "milk-v-pioneer",
    vendor: "milk-v",
    model: "pioneer",
    architecture: Architecture::Riscv64,
    soc: SocId::Sg2042,
    compatibles: &COMPATIBLES,
    boot: BootContract {
        firmware: FirmwareInterface::OpenSbi,
        boot_protocol: BootProtocol::DeviceTreeWithFallbackMap,
        requires_firmware_dtb: true,
        fallback_dts_path: "boards/milk-v/pioneer/milk-v-pioneer.dts",
        kernel_load_base: 0x8020_0000,
    },
    fallback_memory: &MEMORY,
    wiring: WiringLayout {
        pinmux_groups: &[],
        phy_links: &[],
    },
    enabled_drivers: &DRIVERS,
};
