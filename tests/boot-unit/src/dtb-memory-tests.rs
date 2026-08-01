use super::{dtb_memory, MemoryMapEntry, MemoryType, MAX_MEMORY_MAP_ENTRIES};
use vm_fdt::{FdtReserveEntry, FdtWriter};

const RAM_BASE: u64 = 0x8000_0000;
const KERNEL_BASE: usize = 0x8020_0000;
const KERNEL_END: usize = 0x8500_1234;

struct Fixture<'a> {
    ram: &'a [(u64, u64, Option<&'a str>)],
    header_reserved: &'a [(u64, u64)],
    static_reserved: &'a [(u64, u64, bool)],
    dynamic_reserved: bool,
}

fn build_fixture(fixture: Fixture<'_>) -> Vec<u8> {
    let reservations: Vec<_> = fixture
        .header_reserved
        .iter()
        .map(|&(address, size)| FdtReserveEntry::new(address, size).unwrap())
        .collect();
    let mut writer = FdtWriter::new_with_mem_reserv(&reservations).unwrap();
    let root = writer.begin_node("").unwrap();
    writer.property_u32("#address-cells", 2).unwrap();
    writer.property_u32("#size-cells", 2).unwrap();
    for &(base, size, status) in fixture.ram {
        let memory = writer.begin_node(&format!("memory@{base:x}")).unwrap();
        writer.property_string("device_type", "memory").unwrap();
        property_range(&mut writer, "reg", base, size);
        if let Some(status) = status {
            writer.property_string("status", status).unwrap();
        }
        writer.end_node(memory).unwrap();
    }
    if !fixture.static_reserved.is_empty() || fixture.dynamic_reserved {
        let reserved = writer.begin_node("reserved-memory").unwrap();
        writer.property_u32("#address-cells", 2).unwrap();
        writer.property_u32("#size-cells", 2).unwrap();
        writer.property("ranges", &[]).unwrap();
        for &(base, size, enabled) in fixture.static_reserved {
            let node = writer.begin_node(&format!("region@{base:x}")).unwrap();
            property_range(&mut writer, "reg", base, size);
            if !enabled {
                writer.property_string("status", "disabled").unwrap();
            }
            writer.end_node(node).unwrap();
        }
        if fixture.dynamic_reserved {
            let node = writer.begin_node("dynamic").unwrap();
            writer.property_u64("size", 0x20_0000).unwrap();
            writer.end_node(node).unwrap();
        }
        writer.end_node(reserved).unwrap();
    }
    writer.end_node(root).unwrap();
    writer.finish().unwrap()
}

fn property_range(writer: &mut FdtWriter, name: &str, base: u64, size: u64) {
    writer
        .property_array_u32(
            name,
            &[
                (base >> 32) as u32,
                base as u32,
                (size >> 32) as u32,
                size as u32,
            ],
        )
        .unwrap();
}

fn run(fixture: Fixture<'_>) -> Result<Vec<MemoryMapEntry>, dtb_memory::MapError> {
    let bytes = build_fixture(fixture);
    let tree = fdt::Fdt::new(&bytes).unwrap();
    let mut output = [MemoryMapEntry {
        base: 0,
        length: 0,
        ty: MemoryType::Reserved,
    }; MAX_MEMORY_MAP_ENTRIES];
    let count = dtb_memory::build(&tree, KERNEL_BASE, KERNEL_END, &mut output)?;
    Ok(output[..count].to_vec())
}

#[test]
fn two_gibibytes_exposes_more_than_one_gibibyte() {
    let map = run(Fixture {
        ram: &[(RAM_BASE, 2 * 1024 * 1024 * 1024, None)],
        header_reserved: &[],
        static_reserved: &[],
        dynamic_reserved: false,
    })
    .unwrap();
    let usable: usize = map
        .iter()
        .filter(|entry| entry.ty == MemoryType::Usable)
        .map(|entry| entry.length)
        .sum();
    assert!(usable > 1024 * 1024 * 1024);
    assert_eq!(map[0].ty, MemoryType::Bootloader);
    assert_eq!(map[1].ty, MemoryType::Kernel);
    assert_normalized(&map);
}

#[test]
fn reservations_are_never_usable_and_disabled_nodes_are_ignored() {
    let firmware_reserved = (0x8008_0001, 0x7_ffff);
    let header = (0x9000_0001, 0x1f_ffff);
    let static_range = (0xa000_0001, 0x2f_ffff);
    let disabled = (0xb000_0000, 0x40_0000);
    let map = run(Fixture {
        ram: &[(RAM_BASE, 0x8000_0000, None)],
        header_reserved: &[firmware_reserved, header],
        static_reserved: &[
            (static_range.0, static_range.1, true),
            (disabled.0, disabled.1, false),
        ],
        dynamic_reserved: false,
    })
    .unwrap();
    for (start, size) in [firmware_reserved, header, static_range] {
        assert!(!map.iter().any(|entry| {
            entry.ty == MemoryType::Usable
                && entry.base < start as usize + size as usize
                && (start as usize) < entry.base + entry.length
        }));
    }
    assert!(map.iter().any(|entry| {
        entry.ty == MemoryType::Reserved
            && entry.base <= firmware_reserved.0 as usize
            && firmware_reserved.0 as usize + firmware_reserved.1 as usize
                <= entry.base + entry.length
    }));
    assert!(map.iter().any(|entry| {
        entry.ty == MemoryType::Usable
            && entry.base <= disabled.0 as usize
            && disabled.0 as usize + disabled.1 as usize <= entry.base + entry.length
    }));
    assert_normalized(&map);
}

#[test]
fn dynamic_reservation_fails_closed() {
    let result = run(Fixture {
        ram: &[(RAM_BASE, 0x4000_0000, None)],
        header_reserved: &[],
        static_reserved: &[],
        dynamic_reserved: true,
    });
    assert_eq!(
        result.unwrap_err(),
        dtb_memory::MapError::DynamicReservation
    );
}

#[test]
fn kernel_outside_ram_fails_closed() {
    let result = run(Fixture {
        ram: &[(0x4000_0000, 0x1000_0000, None)],
        header_reserved: &[],
        static_reserved: &[],
        dynamic_reserved: false,
    });
    assert_eq!(
        result.unwrap_err(),
        dtb_memory::MapError::KernelOutsideMemory
    );
}

#[test]
fn excessive_splits_fail_instead_of_truncating() {
    let reservations: Vec<_> = (0..40)
        .map(|index| (0x9000_0000 + index * 0x20_0000, 0x10_0000, true))
        .collect();
    let result = run(Fixture {
        ram: &[(RAM_BASE, 0x8000_0000, None)],
        header_reserved: &[],
        static_reserved: &reservations,
        dynamic_reserved: false,
    });
    assert_eq!(result.unwrap_err(), dtb_memory::MapError::TooManyRanges);
}

#[test]
fn only_okay_memory_nodes_are_enabled() {
    for status in ["disabled", "fail", "reserved", "malformed"] {
        let result = run(Fixture {
            ram: &[
                (RAM_BASE, 0x4000_0000, Some(status)),
                (0x1_0000_0000, 0x4000_0000, Some("okay")),
            ],
            header_reserved: &[],
            static_reserved: &[],
            dynamic_reserved: false,
        });
        assert_eq!(
            result.unwrap_err(),
            dtb_memory::MapError::KernelOutsideMemory,
            "status {status:?} must not enable a memory node"
        );
    }
}

fn assert_normalized(map: &[MemoryMapEntry]) {
    assert!(map.iter().any(|entry| entry.ty == MemoryType::Usable));
    for pair in map.windows(2) {
        assert!(pair[0].base + pair[0].length <= pair[1].base);
    }
}
