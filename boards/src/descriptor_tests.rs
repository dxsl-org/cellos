use crate::{
    Architecture, BoardDescriptor, BootContract, BootProtocol, DriverId, FirmwareInterface,
    MemoryRange, MemoryRangeKind, MmioRegion, SocId, ValidationError, WiringLayout,
    MAX_VIRTIO_MMIO_SLOTS,
};

static EMPTY: [&str; 0] = [];
static EMPTY_MEMORY: [MemoryRange; 0] = [];
static EMPTY_WIRING: WiringLayout = WiringLayout {
    pinmux_groups: &[],
    phy_links: &[],
};
static TEST_DRIVERS: [DriverId; 1] = [DriverId::UartNs16550a];
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
        soc: SocId::GenericRiscvVirt,
        compatibles,
        boot: BootContract {
            firmware: FirmwareInterface::OpenSbi,
            boot_protocol: BootProtocol::DeviceTreeWithFallbackMap,
            requires_firmware_dtb: true,
            fallback_dts_path: "boards/test.dts",
            kernel_load_base: memory.first().map_or(0, |range| range.base),
        },
        fallback_memory: memory,
        uart: TEST_MMIO[0],
        plic: Some(MmioRegion {
            irq: None,
            ..TEST_MMIO[0]
        }),
        clint: Some(MmioRegion {
            irq: None,
            ..TEST_MMIO[0]
        }),
        rtc: Some(MmioRegion {
            irq: None,
            ..TEST_MMIO[0]
        }),
        virtio_mmio: &TEST_MMIO,
        wiring: EMPTY_WIRING,
        enabled_drivers: &TEST_DRIVERS,
    }
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
    board.rtc.as_mut().unwrap().size = 0;
    assert_eq!(
        board.validate(),
        Err(ValidationError::ZeroSizedMmioCore("test,mmio"))
    );
}

#[test]
fn rejects_wrong_architecture() {
    static MEMORY: [MemoryRange; 1] = [MemoryRange {
        name: "usable",
        base: 0x8000_0000,
        size: 0x1000,
        kind: MemoryRangeKind::Kernel,
    }];
    let board = descriptor(&TEST_COMPATIBLES, &MEMORY);
    assert_eq!(
        board.validate_for(Architecture::Aarch64),
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
    static VIRTIO: [MmioRegion; MAX_VIRTIO_MMIO_SLOTS + 1] =
        [TEST_MMIO[0]; MAX_VIRTIO_MMIO_SLOTS + 1];
    let mut board = descriptor(&TEST_COMPATIBLES, &MEMORY);
    board.virtio_mmio = &VIRTIO;
    assert_eq!(
        board.validate(),
        Err(ValidationError::TooManyVirtioSlots {
            found: MAX_VIRTIO_MMIO_SLOTS + 1,
            max: MAX_VIRTIO_MMIO_SLOTS,
        })
    );
}

#[test]
fn rejects_duplicate_enabled_drivers() {
    static MEMORY: [MemoryRange; 1] = [MemoryRange {
        name: "usable",
        base: 0x8000_0000,
        size: 0x1000,
        kind: MemoryRangeKind::Usable,
    }];
    static DRIVERS: [DriverId; 2] = [DriverId::UartNs16550a; 2];
    let mut board = descriptor(&TEST_COMPATIBLES, &MEMORY);
    board.enabled_drivers = &DRIVERS;
    assert_eq!(
        board.validate(),
        Err(ValidationError::DuplicateEnabledDriver(
            DriverId::UartNs16550a
        ))
    );
}

#[test]
fn rejects_kernel_load_outside_kernel_fallback() {
    static MEMORY: [MemoryRange; 1] = [MemoryRange {
        name: "kernel",
        base: 0x8000_0000,
        size: 0x1000,
        kind: MemoryRangeKind::Kernel,
    }];
    let mut board = descriptor(&TEST_COMPATIBLES, &MEMORY);
    board.boot.kernel_load_base = 0x9000_0000;
    assert_eq!(
        board.validate(),
        Err(ValidationError::KernelLoadOutsideFallback)
    );
}
