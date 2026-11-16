extern crate self as log;

macro_rules! info {
    ($($arg:tt)*) => {
        let _ = format_args!($($arg)*);
    };
}
pub(crate) use info;

// `#[macro_export]` places `warn!` at the crate root (reached as `log::warn!`);
// a `use` re-export would collide with the built-in `#[warn]` attribute.
#[macro_export]
macro_rules! warn {
    ($($arg:tt)*) => {
        let _ = format_args!($($arg)*);
    };
}

macro_rules! error {
    ($($arg:tt)*) => {
        let _ = format_args!($($arg)*);
    };
}
pub(crate) use error;

/// Host stand-in for the kernel's board selection: the default x86_64 build
/// selects the QEMU q35 descriptor.
#[allow(dead_code)]
mod board {
    use cellos_boards::BoardDescriptor;

    pub(crate) fn active() -> &'static BoardDescriptor {
        &cellos_boards::qemu_q35_x86_64::QEMU_Q35_X86_64
    }
}

/// Host stand-in for `crate::arch::iommu`: the remapper the kernel names per
/// architecture. The host has none, as on aarch64 (`kernel/src/arch/aarch64/iommu.rs`).
mod arch {
    pub mod iommu {
        pub use crate::iommu::NoRemapper as Backend;

        pub const BACKEND: Backend = Backend;
    }
}

#[allow(dead_code)]
#[path = "../../../kernel/src/dev/iommu.rs"]
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
