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
const UART: MmioRegion = MmioRegion {
    compatible: "arm,pl011",
    base: hal_soc_arm_virt::QEMU_ARM_VIRT.uart.mmio.base as u64,
    size: hal_soc_arm_virt::QEMU_ARM_VIRT.uart.mmio.size as u64,
    irq: Some(hal_soc_arm_virt::QEMU_ARM_VIRT.uart.spi),
};
const fn virtio_mmio_slots() -> [MmioRegion; 32] {
    let layout = hal_soc_arm_virt::QEMU_ARM_VIRT.virtio;
    let mut slots = [MmioRegion {
        compatible: "virtio,mmio",
        base: 0,
        size: layout.stride as u64,
        irq: None,
    }; 32];
    let mut index = 0;
    while index < slots.len() {
        slots[index].base = layout.base as u64 + index as u64 * layout.stride as u64;
        slots[index].irq = Some(layout.first_spi + index as u32);
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
        kernel_load_base: 0x4008_0000,
    },
    fallback_memory: &MEMORY,
    uart: UART,
    plic: None,
    clint: None,
    rtc: Some(MmioRegion {
        compatible: "google,goldfish-rtc",
        base: hal_soc_arm_virt::QEMU_ARM_VIRT.rtc.base as u64,
        size: hal_soc_arm_virt::QEMU_ARM_VIRT.rtc.size as u64,
        irq: None,
    }),
    virtio_mmio: &VIRTIO_MMIO,
    wiring: WiringLayout {
        pinmux_groups: &[],
        phy_links: &[],
    },
    enabled_drivers: &DRIVERS,
};
