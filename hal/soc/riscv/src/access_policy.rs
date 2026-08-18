/// UART access policy for a RISC-V SoC profile.
///
/// `SbiDbcnOnly` means the kernel must not probe or map a UART MMIO block.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UartAccessPolicy {
    Mmio,
    SbiDbcnOnly,
}

/// RTC access policy for a RISC-V SoC profile.
///
/// `Unavailable` keeps kernel consumers fail-closed by preserving a zero RTC
/// base rather than probing unsupported registers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RtcAccessPolicy {
    Mmio,
    Unavailable,
}

/// VirtIO MMIO discovery policy for a RISC-V SoC profile.
///
/// `Absent` means DTB discovery should be skipped and all slots remain empty.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VirtioMmioPolicy {
    Discover,
    Absent,
}
