use crate::{
    Architecture, BoardDescriptor, BootContract, BootProtocol, DriverId, FirmwareInterface,
    MemoryRange, MemoryRangeKind, MmioRegion, SocId, WiringLayout,
};

const COMPATIBLES: [&str; 2] = ["starfive,visionfive-2-v1.3b", "starfive,jh7110"];
const DRIVERS: [DriverId; 4] = [
    DriverId::UartNs16550a,
    DriverId::PlicSifive,
    DriverId::ClintSifive,
    DriverId::SdhciDwCqe,
];
const MEMORY: [MemoryRange; 3] = [
    MemoryRange {
        name: "opensbi",
        base: 0x4000_0000,
        size: 0x0020_0000,
        kind: MemoryRangeKind::Bootloader,
    },
    MemoryRange {
        name: "kernel",
        base: 0x4020_0000,
        size: 0x0400_0000,
        kind: MemoryRangeKind::Kernel,
    },
    MemoryRange {
        name: "usable",
        base: 0x4420_0000,
        size: 0x0BE0_0000,
        kind: MemoryRangeKind::Usable,
    },
];

pub const STARFIVE_VISIONFIVE_2: BoardDescriptor = BoardDescriptor {
    slug: "starfive-visionfive-2",
    vendor: "starfive",
    model: "visionfive-2",
    architecture: Architecture::Riscv64,
    soc: SocId::Jh7110,
    compatibles: &COMPATIBLES,
    boot: BootContract {
        firmware: FirmwareInterface::OpenSbi,
        boot_protocol: BootProtocol::DeviceTreeWithFallbackMap,
        requires_firmware_dtb: true,
        fallback_dts_path: "boards/starfive/visionfive-2/starfive-visionfive-2.dts",
    },
    fallback_memory: &MEMORY,
    uart: MmioRegion {
        compatible: "snps,dw-apb-uart",
        base: 0x1000_0000,
        size: 0x100,
        irq: Some(10),
    },
    plic: Some(MmioRegion {
        compatible: "sifive,plic-1.0.0",
        base: 0x0C00_0000,
        size: 0x0400_0000,
        irq: None,
    }),
    clint: Some(MmioRegion {
        compatible: "sifive,clint0",
        base: 0x0200_0000,
        size: 0x0001_0000,
        irq: None,
    }),
    rtc: None,
    virtio_mmio: &[],
    wiring: WiringLayout {
        pinmux_groups: &[],
        phy_links: &["sdio1"],
    },
    enabled_drivers: &DRIVERS,
};
