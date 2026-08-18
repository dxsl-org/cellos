use crate::{
    Architecture, BoardDescriptor, BootContract, BootProtocol, DriverId, FirmwareInterface,
    MemoryRange, MemoryRangeKind, MmioRegion, SocId, WiringLayout,
};

const COMPATIBLES: [&str; 1] = ["linux,dummy-virt"];
const DRIVERS: [DriverId; 5] = [
    DriverId::UartPl011,
    DriverId::GicV2,
    DriverId::RtcGoldfish,
    DriverId::VirtioMmio,
    DriverId::PcieEcam,
];
const MEMORY: [MemoryRange; 2] = [
    MemoryRange {
        name: "kernel",
        base: 0x4000_0000,
        size: 0x0200_0000,
        kind: MemoryRangeKind::Kernel,
    },
    MemoryRange {
        name: "usable",
        base: 0x4200_0000,
        size: 0x0E00_0000,
        kind: MemoryRangeKind::Usable,
    },
];
const UART: MmioRegion = MmioRegion {
    compatible: "arm,pl011",
    base: 0x0900_0000,
    size: 0x1000,
    irq: Some(1),
};
const fn virtio_mmio_slots() -> [MmioRegion; 32] {
    let mut slots = [MmioRegion {
        compatible: "virtio,mmio",
        base: 0,
        size: 0x200,
        irq: None,
    }; 32];
    let mut index = 0;
    while index < slots.len() {
        slots[index].base = 0x0A00_0000 + index as u64 * 0x200;
        slots[index].irq = Some(16 + index as u32);
        index += 1;
    }
    slots
}
const VIRTIO_MMIO: [MmioRegion; 32] = virtio_mmio_slots();

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
    },
    fallback_memory: &MEMORY,
    uart: UART,
    plic: None,
    clint: None,
    rtc: Some(MmioRegion {
        compatible: "google,goldfish-rtc",
        base: 0x0902_0000,
        size: 0x1000,
        irq: None,
    }),
    virtio_mmio: &VIRTIO_MMIO,
    wiring: WiringLayout {
        pinmux_groups: &[],
        phy_links: &[],
    },
    enabled_drivers: &DRIVERS,
};
