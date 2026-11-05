use super::*;
use crate::pte::{PAGE_RW, PPGTT_PTE_PAT1};
use std::collections::HashMap;

/// In-memory tables at fake physical addresses starting at 1 MiB.
#[derive(Default)]
struct Store {
    tables: HashMap<u64, [u64; ENTRIES]>,
    next: u64,
    limit: Option<usize>,
}

impl TableStore for Store {
    fn alloc_table(&mut self) -> Option<u64> {
        if self.limit.is_some_and(|l| self.tables.len() >= l) {
            return None;
        }
        let pa = 0x10_0000 + self.next * 0x1000;
        self.next += 1;
        self.tables.insert(pa, [0; ENTRIES]);
        Some(pa)
    }
    fn read(&self, table: u64, index: usize) -> u64 {
        self.tables[&table][index]
    }
    fn write(&mut self, table: u64, index: usize, value: u64) {
        self.tables.get_mut(&table).unwrap()[index] = value;
    }
}

fn vm() -> (Store, Ppgtt) {
    let mut store = Store::default();
    let vm = Ppgtt::new(&mut store, Layout::PTL).unwrap();
    (store, vm)
}

#[test]
fn mapped_page_translates_with_offset() {
    let (mut s, mut vm) = vm();
    vm.map_4k(&mut s, 0x7f00_1000, 0x2345_6000, PatIndex::WB, true)
        .unwrap();
    assert_eq!(
        vm.translate(&mut s, 0x7f00_1abc).unwrap(),
        Some(0x2345_6abc)
    );
    assert_eq!(vm.translate(&mut s, 0x7f00_2000).unwrap(), None);
}

#[test]
fn ptl_layout_builds_five_levels_with_root_index_zero() {
    let (mut s, mut vm) = vm();
    vm.map_4k(&mut s, 0x0000_7fff_ffff_f000, 0x1000, PatIndex::WB, true)
        .unwrap();
    // root + level 3, 2, 1, 0 tables.
    assert_eq!(s.tables.len(), 5);
    assert_ne!(s.read(vm.root(), 0) & PAGE_PRESENT, 0);
    assert_eq!(
        (1..ENTRIES).filter(|&i| s.read(vm.root(), i) != 0).count(),
        0
    );
}

#[test]
fn neighbouring_pages_share_intermediate_tables() {
    let (mut s, mut vm) = vm();
    vm.map_4k(&mut s, 0x20_0000, 0x1000, PatIndex::WB, true)
        .unwrap();
    vm.map_4k(&mut s, 0x20_1000, 0x2000, PatIndex::WB, true)
        .unwrap();
    assert_eq!(s.tables.len(), 5);
    // A page in another 2 MiB region needs one new level-0 table only.
    vm.map_4k(&mut s, 0x40_0000, 0x3000, PatIndex::WB, true)
        .unwrap();
    assert_eq!(s.tables.len(), 6);
}

#[test]
fn leaf_bits_follow_the_encoder() {
    let (mut s, mut vm) = vm();
    vm.map_4k(&mut s, 0x1000, 0x5000, PatIndex::WB, true)
        .unwrap();
    let leaf_table = s.tables.keys().copied().max().unwrap();
    assert_eq!(
        s.read(leaf_table, 1),
        0x5000 | PAGE_PRESENT | PAGE_RW | PPGTT_PTE_PAT1
    );
}

#[test]
fn mapping_twice_is_refused_and_keeps_the_first_page() {
    let (mut s, mut vm) = vm();
    vm.map_4k(&mut s, 0x1000, 0x5000, PatIndex::WB, true)
        .unwrap();
    assert_eq!(
        vm.map_4k(&mut s, 0x1000, 0x6000, PatIndex::WB, true),
        Err(MapError::AlreadyMapped)
    );
    assert_eq!(vm.translate(&mut s, 0x1000).unwrap(), Some(0x5000));
}

#[test]
fn unmap_returns_the_page_and_clears_translation() {
    let (mut s, mut vm) = vm();
    vm.map_4k(&mut s, 0x3000, 0x9000, PatIndex::WB, false)
        .unwrap();
    assert_eq!(vm.unmap_4k(&mut s, 0x3000), Ok(0x9000));
    assert_eq!(vm.translate(&mut s, 0x3000).unwrap(), None);
    assert_eq!(vm.unmap_4k(&mut s, 0x3000), Err(MapError::NotMapped));
    assert_eq!(vm.unmap_4k(&mut s, 0x8000_0000), Err(MapError::NotMapped));
}

#[test]
fn virtual_addresses_outside_48_bits_or_unaligned_are_refused() {
    let (mut s, mut vm) = vm();
    assert_eq!(
        vm.map_4k(&mut s, 1 << 48, 0x1000, PatIndex::WB, true),
        Err(MapError::BadVirtual)
    );
    assert_eq!(
        vm.map_4k(&mut s, 0x1001, 0x1000, PatIndex::WB, true),
        Err(MapError::BadVirtual)
    );
}

#[test]
fn physical_pages_beyond_dma_width_are_refused_before_any_table_is_built() {
    let (mut s, mut vm) = vm();
    let err = vm.map_4k(&mut s, 0x1000, 1 << 46, PatIndex::WB, true);
    assert_eq!(err, Err(MapError::Encode(EncodeError::AddressTooWide)));
    assert_eq!(s.tables.len(), 1);
}

#[test]
fn table_exhaustion_is_reported() {
    let mut s = Store {
        limit: Some(3),
        ..Store::default()
    };
    let mut vm = Ppgtt::new(&mut s, Layout::PTL).unwrap();
    assert_eq!(
        vm.map_4k(&mut s, 0x1000, 0x1000, PatIndex::WB, true),
        Err(MapError::OutOfTables)
    );
}

#[test]
fn huge_leaf_in_the_walk_blocks_a_4k_mapping() {
    let (mut s, mut vm) = vm();
    vm.map_4k(&mut s, 0x1000, 0x1000, PatIndex::WB, true)
        .unwrap();
    // Replace the level-1 entry with a 2 MiB leaf, as a later huge-page path would.
    let mut walk = vm.root();
    for level in (2..=4).rev() {
        walk = s.read(walk, index(0x1000, level)) & ADDR_MASK_4K;
    }
    s.write(
        walk,
        0,
        crate::pte::encode_leaf(0, 1, PatIndex::WB, true, 46).unwrap(),
    );
    assert_eq!(
        vm.map_4k(&mut s, 0x2000, 0x2000, PatIndex::WB, true),
        Err(MapError::AlreadyMapped)
    );
}

#[test]
fn invalid_layouts_are_rejected() {
    let mut s = Store::default();
    let bad = Layout {
        va_bits: 60,
        max_level: 3,
        dma_bits: 46,
    };
    assert!(matches!(Ppgtt::new(&mut s, bad), Err(MapError::BadLayout)));
    let ok = Layout {
        va_bits: 48,
        max_level: 3,
        dma_bits: 46,
    };
    assert!(Ppgtt::new(&mut s, ok).is_ok());
}
