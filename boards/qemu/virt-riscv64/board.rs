use crate::{
    Architecture, BoardDescriptor, BootContract, BootProtocol, FirmwareInterface, MemoryRange,
    MemoryRangeKind, MmioRegion, WiringLayout,
};

const BOARD_COMPATIBLES: [&str; 2] = ["riscv-virtio", "qemu,virt"];
const ENABLED_DRIVERS: [&str; 5] = [
    "uart-ns16550a",
    "plic-sifive",
    "clint-sifive",
    "rtc-goldfish",
    "virtio-mmio",
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
const UART: MmioRegion = MmioRegion {
    compatible: "ns16550a",
    base: 0x1000_0000,
    size: 0x100,
    irq: Some(10),
};
const PLIC: MmioRegion = MmioRegion {
    compatible: "sifive,plic-1.0.0",
    base: 0x0C00_0000,
    size: 0x0400_0000,
    irq: None,
};
const CLINT: MmioRegion = MmioRegion {
    compatible: "sifive,clint0",
    base: 0x0200_0000,
    size: 0x0001_0000,
    irq: None,
};
const RTC: MmioRegion = MmioRegion {
    compatible: "google,goldfish-rtc",
    base: 0x0010_1000,
    size: 0x1000,
    irq: None,
};
const VIRTIO_MMIO: [MmioRegion; 5] = [
    MmioRegion {
        compatible: "virtio,mmio",
        base: 0x1000_1000,
        size: 0x1000,
        irq: Some(1),
    },
    MmioRegion {
        compatible: "virtio,mmio",
        base: 0x1000_2000,
        size: 0x1000,
        irq: Some(2),
    },
    MmioRegion {
        compatible: "virtio,mmio",
        base: 0x1000_3000,
        size: 0x1000,
        irq: Some(3),
    },
    MmioRegion {
        compatible: "virtio,mmio",
        base: 0x1000_4000,
        size: 0x1000,
        irq: Some(4),
    },
    MmioRegion {
        compatible: "virtio,mmio",
        base: 0x1000_5000,
        size: 0x1000,
        irq: Some(5),
    },
];

pub const QEMU_VIRT_RISCV64: BoardDescriptor = BoardDescriptor {
    slug: "qemu-virt-riscv64",
    vendor: "qemu",
    model: "virt-riscv64",
    architecture: Architecture::Riscv64,
    compatibles: &BOARD_COMPATIBLES,
    boot: BootContract {
        firmware: FirmwareInterface::OpenSbi,
        boot_protocol: BootProtocol::DeviceTreeWithFallbackMap,
        requires_firmware_dtb: false,
        fallback_dts_path: "boards/qemu/virt-riscv64/qemu-virt-riscv64.dts",
    },
    fallback_memory: &FALLBACK_MEMORY,
    uart: UART,
    plic: PLIC,
    clint: CLINT,
    rtc: RTC,
    virtio_mmio: &VIRTIO_MMIO,
    wiring: EMPTY_WIRING,
    enabled_drivers: &ENABLED_DRIVERS,
};
