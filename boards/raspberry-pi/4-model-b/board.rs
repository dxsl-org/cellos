use crate::{
    Architecture, BoardDescriptor, BootContract, BootProtocol, DriverId, FirmwareInterface,
    MemoryRange, MemoryRangeKind, MmioRegion, SocId, WiringLayout,
};

const COMPATIBLES: [&str; 2] = ["raspberrypi,4-model-b", "brcm,bcm2711"];
const DRIVERS: [DriverId; 6] = [
    DriverId::UartPl011,
    DriverId::GicV2,
    DriverId::TimerBcm2835System,
    DriverId::SdhciArasan,
    DriverId::GpioBcm2711,
    DriverId::PcieEcam,
];
const MEMORY: [MemoryRange; 2] = [
    MemoryRange {
        name: "kernel",
        base: 0x0008_0000,
        size: 0x0100_0000,
        kind: MemoryRangeKind::Kernel,
    },
    MemoryRange {
        name: "usable-low-memory",
        base: 0x0108_0000,
        size: 0x3DF8_0000,
        kind: MemoryRangeKind::Usable,
    },
];

pub const RASPBERRY_PI_4_MODEL_B: BoardDescriptor = BoardDescriptor {
    slug: "raspberry-pi-4-model-b",
    vendor: "raspberry-pi",
    model: "4-model-b",
    architecture: Architecture::Aarch64,
    soc: SocId::Bcm2711,
    compatibles: &COMPATIBLES,
    boot: BootContract {
        firmware: FirmwareInterface::VideoCore,
        boot_protocol: BootProtocol::DeviceTreeWithFallbackMap,
        requires_firmware_dtb: true,
        fallback_dts_path: "boards/raspberry-pi/4-model-b/raspberry-pi-4-model-b.dts",
    },
    fallback_memory: &MEMORY,
    uart: MmioRegion {
        compatible: "arm,pl011",
        base: 0xFE20_1000,
        size: 0x1000,
        irq: None,
    },
    plic: None,
    clint: None,
    rtc: None,
    virtio_mmio: &[],
    wiring: WiringLayout {
        pinmux_groups: &["uart-gpio14-15-alt0", "emmc2-gpio48-53-alt3"],
        phy_links: &["bcm54213pe-gigabit-ethernet"],
    },
    enabled_drivers: &DRIVERS,
};
