//! The probed controller, as [`crate::NicDevice::init`] sees it.

use ostd::io::print_fmt;
use ostd::mmio::{self, MmioRegion};
use ostd::syscall::{sys_exit, PcieDeviceInfo};

/// The controller `FindPcieDeviceByVendor` handed this cell. The cell owns its
/// BDF from here on; BARs are claimed through it.
pub struct Probe {
    tag: &'static str,
    info: PcieDeviceInfo,
}

impl Probe {
    pub(crate) fn new(tag: &'static str, info: PcieDeviceInfo) -> Self {
        Self { tag, info }
    }

    /// The 424 device record.
    pub fn info(&self) -> &PcieDeviceInfo {
        &self.info
    }

    /// PCIe requester ID of the owned function.
    pub fn bdf(&self) -> u32 {
        self.info.bdf
    }

    /// Claim BAR0 from the 424 record: at most `cap` bytes (all of `cap` when
    /// the record reports no length), and at least `min`, the end of the
    /// highest register the chip touches. Exits 1 when the BAR is unusable or
    /// the claim is refused.
    pub fn claim_bar0(&self, cap: usize, min: usize) -> MmioRegion {
        let base = self.info.bar0_base as usize;
        let Some(len) = bar0_window(base, self.info.bar0_len as usize, cap, min) else {
            let len = bar0_len(self.info.bar0_len as usize, cap);
            self.fail(format_args!(
                "controller has no usable MMIO BAR (base={:#x} len={:#x})",
                base, len
            ));
        };
        match mmio::request_region(base, len) {
            Ok(region) => region,
            Err(_) => sys_exit(1),
        }
    }

    /// Print `"{TAG} {what}"` and exit 1: bring-up cannot continue.
    pub fn fail(&self, what: core::fmt::Arguments<'_>) -> ! {
        let _ = print_fmt(format_args!("{} {}\n", self.tag, what));
        sys_exit(1)
    }
}

fn bar0_len(reported: usize, cap: usize) -> usize {
    match reported {
        0 => cap,
        len => len.min(cap),
    }
}

/// The BAR0 length to claim, or `None` when the BAR is absent or too short.
fn bar0_window(base: usize, reported: usize, cap: usize, min: usize) -> Option<usize> {
    let len = bar0_len(reported, cap);
    (base != 0 && len >= min).then_some(len)
}

#[cfg(test)]
mod tests {
    use super::bar0_window;

    #[test]
    fn bar0_is_capped_defaulted_and_refused_when_short_or_absent() {
        const CAP: usize = 0x2_0000;
        const MIN: usize = 0x0_E040;
        assert_eq!(bar0_window(0xFEB0_0000, 0x8_0000, CAP, MIN), Some(CAP));
        assert_eq!(bar0_window(0xFEB0_0000, 0x1_0000, CAP, MIN), Some(0x1_0000));
        assert_eq!(
            bar0_window(0xFEB0_0000, 0, CAP, MIN),
            Some(CAP),
            "unreported"
        );
        assert_eq!(bar0_window(0xFEB0_0000, 0x4000, CAP, MIN), None, "short");
        assert_eq!(bar0_window(0, 0x2_0000, CAP, MIN), None, "I/O or absent");
    }
}
