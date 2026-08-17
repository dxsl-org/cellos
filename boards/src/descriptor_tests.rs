use crate::qemu_virt_riscv64::QEMU_VIRT_RISCV64;
use crate::{
    Architecture, BoardDescriptor, BootContract, BootProtocol, FirmwareInterface, MemoryRange,
    MemoryRangeKind, MmioRegion, ValidationError, WiringLayout, MAX_VIRTIO_MMIO_SLOTS,
};

static EMPTY: [&str; 0] = [];
static EMPTY_MEMORY: [MemoryRange; 0] = [];
static EMPTY_WIRING: WiringLayout = WiringLayout {
    pinmux_groups: &[],
    phy_links: &[],
};
static TEST_COMPATIBLES: [&str; 1] = ["test,board"];
const TEST_MMIO: [MmioRegion; 1] = [MmioRegion {
    compatible: "test,mmio",
    base: 0x1000_0000,
    size: 0x1000,
    irq: Some(1),
}];

fn descriptor(
    compatibles: &'static [&'static str],
    memory: &'static [MemoryRange],
) -> BoardDescriptor {
    BoardDescriptor {
        slug: "test",
        vendor: "cellos",
        model: "test",
        architecture: Architecture::Riscv64,
        compatibles,
        boot: BootContract {
            firmware: FirmwareInterface::OpenSbi,
            boot_protocol: BootProtocol::DeviceTreeWithFallbackMap,
            requires_firmware_dtb: true,
            fallback_dts_path: "boards/test.dts",
        },
        fallback_memory: memory,
        uart: TEST_MMIO[0],
        plic: MmioRegion {
            irq: None,
            ..TEST_MMIO[0]
        },
        clint: MmioRegion {
            irq: None,
            ..TEST_MMIO[0]
        },
        rtc: MmioRegion {
            irq: None,
            ..TEST_MMIO[0]
        },
        virtio_mmio: &TEST_MMIO,
        wiring: EMPTY_WIRING,
        enabled_drivers: &["test-driver"],
    }
}

#[test]
fn qemu_descriptor_matches_current_kernel_constants() {
    assert_eq!(QEMU_VIRT_RISCV64.architecture, Architecture::Riscv64);
    assert_eq!(
        QEMU_VIRT_RISCV64.compatibles,
        &["riscv-virtio", "qemu,virt"]
    );
    assert_eq!(QEMU_VIRT_RISCV64.boot.firmware, FirmwareInterface::OpenSbi);
    assert!(!QEMU_VIRT_RISCV64.boot.requires_firmware_dtb);
    assert_eq!(
        QEMU_VIRT_RISCV64.boot.fallback_dts_path,
        "boards/qemu/virt-riscv64/qemu-virt-riscv64.dts"
    );
    assert_eq!(QEMU_VIRT_RISCV64.fallback_memory[0].base, 0x8000_0000);
    assert_eq!(QEMU_VIRT_RISCV64.fallback_memory[1].base, 0x8020_0000);
    assert_eq!(QEMU_VIRT_RISCV64.fallback_memory[2].base, 0x8420_0000);
    assert_eq!(QEMU_VIRT_RISCV64.uart.base, 0x1000_0000);
    assert_eq!(QEMU_VIRT_RISCV64.uart.irq, Some(10));
    assert_eq!(QEMU_VIRT_RISCV64.plic.base, 0x0C00_0000);
    assert_eq!(QEMU_VIRT_RISCV64.plic.size, 0x0400_0000);
    assert_eq!(QEMU_VIRT_RISCV64.clint.base, 0x0200_0000);
    assert_eq!(QEMU_VIRT_RISCV64.rtc.base, 0x0010_1000);
    assert_eq!(QEMU_VIRT_RISCV64.virtio_mmio.len(), 5);
    assert_eq!(QEMU_VIRT_RISCV64.virtio_mmio[4].base, 0x1000_5000);
    assert_eq!(QEMU_VIRT_RISCV64.virtio_mmio[4].irq, Some(5));
    assert_eq!(
        QEMU_VIRT_RISCV64.enabled_drivers,
        &[
            "uart-ns16550a",
            "plic-sifive",
            "clint-sifive",
            "rtc-goldfish",
            "virtio-mmio"
        ]
    );
    assert_eq!(
        QEMU_VIRT_RISCV64.validate_for(Architecture::Riscv64),
        Ok(())
    );
}

#[test]
fn rejects_empty_compatibles() {
    let board = descriptor(&EMPTY, &EMPTY_MEMORY);
    assert_eq!(board.validate(), Err(ValidationError::EmptyCompatibles));
}

#[test]
fn rejects_overlapping_fallback_ranges() {
    static MEMORY: [MemoryRange; 2] = [
        MemoryRange {
            name: "boot",
            base: 0x8000_0000,
            size: 0x0020_0000,
            kind: MemoryRangeKind::Bootloader,
        },
        MemoryRange {
            name: "kernel",
            base: 0x8010_0000,
            size: 0x0400_0000,
            kind: MemoryRangeKind::Kernel,
        },
    ];
    let board = descriptor(&TEST_COMPATIBLES, &MEMORY);
    assert_eq!(
        board.validate(),
        Err(ValidationError::OverlappingFallbackRange("boot", "kernel"))
    );
}

#[test]
fn rejects_zero_sized_fallback_ranges() {
    static MEMORY: [MemoryRange; 1] = [MemoryRange {
        name: "usable",
        base: 0x8420_0000,
        size: 0,
        kind: MemoryRangeKind::Usable,
    }];
    let board = descriptor(&TEST_COMPATIBLES, &MEMORY);
    assert_eq!(
        board.validate(),
        Err(ValidationError::ZeroSizedFallbackRange("usable"))
    );
}

#[test]
fn rejects_zero_sized_core_mmio_regions() {
    static MEMORY: [MemoryRange; 1] = [MemoryRange {
        name: "usable",
        base: 0x8420_0000,
        size: 0x1000,
        kind: MemoryRangeKind::Usable,
    }];
    let mut board = descriptor(&TEST_COMPATIBLES, &MEMORY);
    board.rtc.size = 0;
    assert_eq!(
        board.validate(),
        Err(ValidationError::ZeroSizedMmioCore("test,mmio"))
    );
}

#[test]
fn rejects_wrong_architecture() {
    assert_eq!(
        QEMU_VIRT_RISCV64.validate_for(Architecture::Aarch64),
        Err(ValidationError::ArchitectureMismatch {
            expected: Architecture::Aarch64,
            found: Architecture::Riscv64,
        })
    );
}

#[test]
fn rejects_overflowing_fallback_ranges() {
    static MEMORY: [MemoryRange; 1] = [MemoryRange {
        name: "overflow",
        base: u64::MAX - 1,
        size: 2,
        kind: MemoryRangeKind::Reserved,
    }];
    let board = descriptor(&TEST_COMPATIBLES, &MEMORY);
    assert_eq!(
        board.validate(),
        Err(ValidationError::OverflowingFallbackRange("overflow"))
    );
}

#[test]
fn rejects_more_virtio_slots_than_kernel_capacity() {
    static MEMORY: [MemoryRange; 1] = [MemoryRange {
        name: "usable",
        base: 0x8000_0000,
        size: 0x1000,
        kind: MemoryRangeKind::Usable,
    }];
    static VIRTIO: [MmioRegion; MAX_VIRTIO_MMIO_SLOTS + 1] = [TEST_MMIO[0]; 9];
    let mut board = descriptor(&TEST_COMPATIBLES, &MEMORY);
    board.virtio_mmio = &VIRTIO;
    assert_eq!(
        board.validate(),
        Err(ValidationError::TooManyVirtioSlots { found: 9, max: 8 })
    );
}
