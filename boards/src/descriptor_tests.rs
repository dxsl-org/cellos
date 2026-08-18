use crate::{
    Architecture, BoardDescriptor, BootContract, BootProtocol, DriverId, FirmwareInterface,
    MemoryRange, MemoryRangeKind, SocId, ValidationError, WiringLayout,
};

static EMPTY: [&str; 0] = [];
static EMPTY_MEMORY: [MemoryRange; 0] = [];
static EMPTY_WIRING: WiringLayout = WiringLayout {
    pinmux_groups: &[],
    phy_links: &[],
};
static TEST_DRIVERS: [DriverId; 1] = [DriverId::UartNs16550a];
static TEST_COMPATIBLES: [&str; 1] = ["test,board"];
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
fn rejects_missing_fallback_dts_for_device_tree_boot() {
    static MEMORY: [MemoryRange; 1] = [MemoryRange {
        name: "kernel",
        base: 0x8000_0000,
        size: 0x1000,
        kind: MemoryRangeKind::Kernel,
    }];
    let mut board = descriptor(&TEST_COMPATIBLES, &MEMORY);
    board.boot.fallback_dts_path = "";
    assert_eq!(board.validate(), Err(ValidationError::MissingFallbackDts));
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
