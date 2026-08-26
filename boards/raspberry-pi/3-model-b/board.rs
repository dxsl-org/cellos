use crate::{
    Architecture, BoardDescriptor, BootContract, BootProtocol, DriverId, FirmwareInterface,
    MemoryRange, MemoryRangeKind, SocId, WiringLayout,
};

const BOARD_COMPATIBLES: [&str; 2] = ["raspberrypi,3-model-b", "brcm,bcm2837"];
const ENABLED_DRIVERS: [DriverId; 11] = [
    DriverId::UartBcmMini,
    DriverId::IrqBcm2836Local,
    DriverId::IrqBcm2835Legacy,
    DriverId::TimerBcm2835System,
    DriverId::SdhciArasan,
    DriverId::I2cBcmBsc,
    DriverId::SpiBcm0,
    DriverId::GpioBcm2837,
    DriverId::DisplayBcmMailbox,
    DriverId::UsbDwc2,
    DriverId::EthernetLan9514,
];
const PINMUX_GROUPS: [&str; 4] = [
    "uart-gpio14-15-alt5",
    "sd-gpio48-53-alt3",
    "i2c1-gpio2-3-alt0",
    "spi0-gpio7-11-alt0",
];
const WIRING: WiringLayout = WiringLayout {
    pinmux_groups: &PINMUX_GROUPS,
    phy_links: &[],
};
const FALLBACK_MEMORY: [MemoryRange; 2] = [
    MemoryRange {
        name: "kernel",
        base: 0x0008_0000,
        size: 0x0100_0000,
        kind: MemoryRangeKind::Kernel,
    },
    // Firmware DTB memory remains authoritative. This recovery ceiling leaves
    // the top 64 MiB below the BCM2837 peripheral aperture to VideoCore.
    MemoryRange {
        name: "usable",
        base: 0x0108_0000,
        size: 0x39F8_0000,
        kind: MemoryRangeKind::Usable,
    },
];
pub const RASPBERRY_PI_3_MODEL_B: BoardDescriptor = BoardDescriptor {
    slug: "raspberry-pi-3-model-b",
    vendor: "raspberry-pi",
    model: "3-model-b",
    architecture: Architecture::Aarch64,
    soc: SocId::Bcm2837,
    compatibles: &BOARD_COMPATIBLES,
    boot: BootContract {
        firmware: FirmwareInterface::VideoCore,
        boot_protocol: BootProtocol::DeviceTreeWithFallbackMap,
        requires_firmware_dtb: false,
        fallback_dts_path: "boards/raspberry-pi/3-model-b/raspberry-pi-3-model-b.dts",
        kernel_load_base: 0x0008_0000,
    },
    fallback_memory: &FALLBACK_MEMORY,
    wiring: WIRING,
    enabled_drivers: &ENABLED_DRIVERS,
};
