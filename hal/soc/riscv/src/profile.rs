use crate::{RtcAccessPolicy, UartAccessPolicy, VirtioMmioPolicy};

/// Immutable SoC-profile facts used by the RV64 platform path.
///
/// Compatible lists remain static slices so early boot can borrow them without
/// allocation. Policies must fail closed when hardware access is unsupported.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RiscvSocProfile {
    pub slug: &'static str,
    pub uart_compatibles: &'static [&'static str],
    pub plic_compatibles: &'static [&'static str],
    pub clint_compatibles: &'static [&'static str],
    pub rtc_compatibles: &'static [&'static str],
    pub uart_access: UartAccessPolicy,
    pub rtc_access: RtcAccessPolicy,
    pub virtio_mmio: VirtioMmioPolicy,
}

impl RiscvSocProfile {
    /// Returns true when the profile allows UART MMIO probing.
    pub const fn allows_uart_mmio(self) -> bool {
        matches!(self.uart_access, UartAccessPolicy::Mmio)
    }

    /// Returns true when the profile allows RTC MMIO probing.
    pub const fn allows_rtc_mmio(self) -> bool {
        matches!(self.rtc_access, RtcAccessPolicy::Mmio)
    }

    /// Returns true when the profile should scan DTB VirtIO MMIO nodes.
    pub const fn discovers_virtio_mmio(self) -> bool {
        matches!(self.virtio_mmio, VirtioMmioPolicy::Discover)
    }
}
