extern crate self as log;

macro_rules! info {
    ($($arg:tt)*) => {
        let _ = format_args!($($arg)*);
    };
}
pub(crate) use info;

#[allow(dead_code)]
mod iommu_x86 {
    use super::iommu::DmaMapResult;

    pub fn init_hw() {}
    pub fn is_present() -> bool {
        false
    }
    pub fn map_range_for_cell(_tid: u64, _bdf: u32, phys: u64, _size: usize) -> DmaMapResult {
        DmaMapResult::Mapped(phys)
    }
    pub fn unmap_cell_domain(_tid: u64) -> bool {
        true
    }
    pub fn activate() {}
}

#[allow(dead_code)]
#[path = "../../../kernel/src/task/drivers/iommu.rs"]
mod iommu;

use iommu::{classify_dma_publication, DmaMapResult};

#[test]
fn acknowledged_context_publication_returns_mapped_iova() {
    assert_eq!(
        classify_dma_publication(0x8123_4000, true, true),
        DmaMapResult::Mapped(0x8123_4000)
    );
}

#[test]
fn command_queue_full_retains_published_mapping_pin() {
    assert_eq!(
        classify_dma_publication(0x8123_4000, false, false),
        DmaMapResult::PublishedUnconfirmed
    );
}

#[test]
fn iofence_timeout_retains_published_mapping_pin() {
    assert_eq!(
        classify_dma_publication(0x8123_4000, true, false),
        DmaMapResult::PublishedUnconfirmed
    );
}
