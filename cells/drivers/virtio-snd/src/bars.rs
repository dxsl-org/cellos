//! [`BarMapper`] over the kernel MMIO Resource Registry.

use abi::pcie_function::PCIE_FUNCTION_BARS;
use core::ptr::NonNull;
use ostd::mmio::{request_region, MmioRegion};
use virtio_pci_transport::BarMapper;

/// MMIO claims for the BARs the transport maps. The claims must outlive the
/// transport, so the owner declares this after the transport (fields drop in
/// declaration order: the device is reset before its BARs are released).
pub struct Bars {
    regions: [Option<MmioRegion>; PCIE_FUNCTION_BARS],
}

impl Bars {
    pub const fn new() -> Self {
        Self {
            regions: [const { None }; PCIE_FUNCTION_BARS],
        }
    }
}

impl BarMapper for Bars {
    /// Claims `[base, base + len)` exclusively; in the single address space
    /// the mapping's address is `base`. The transport, not this cell,
    /// dereferences it.
    fn map(&mut self, bar: u8, base: u64, len: u64) -> Option<NonNull<u8>> {
        let slot = self.regions.get_mut(usize::from(bar))?;
        let start = usize::try_from(base).ok()?;
        let size = usize::try_from(len).ok()?;
        let region = match request_region(start, size) {
            Ok(region) => region,
            Err(error) => {
                let _ = ostd::io::print_fmt(format_args!(
                    "[virtio-snd] BAR{bar} claim {base:#x}+{len:#x} failed: {error:?}\n"
                ));
                return None;
            }
        };
        let ptr = NonNull::new(region.base() as *mut u8)?;
        *slot = Some(region);
        Some(ptr)
    }
}
