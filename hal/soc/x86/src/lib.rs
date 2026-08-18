#![no_std]

//! Immutable x86 machine facts owned by `hal/soc/x86`.
//!
//! ACPI-discovered LAPIC, IOAPIC, HPET, and PCIe ECAM addresses deliberately
//! do not appear here: invalid or missing firmware must keep those gates closed.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// A half-open physical or port-address range.
pub struct AddressRange {
    pub base: usize,
    pub size: usize,
}

impl AddressRange {
    /// Returns the exclusive end, or `None` when the range overflows.
    pub const fn end(self) -> Option<usize> {
        self.base.checked_add(self.size)
    }

    /// Reports whether the non-empty candidate range is fully contained.
    pub const fn contains(self, base: usize, size: usize) -> bool {
        let Some(end) = base.checked_add(size) else {
            return false;
        };
        let Some(limit) = self.end() else {
            return false;
        };
        size != 0 && base >= self.base && end <= limit
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Static port-I/O wiring for one legacy device.
pub struct PortIoDevice {
    pub base: u16,
    pub irq: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Immutable x86 machine facts consumed before ACPI discovery succeeds.
pub struct X86PlatformProfile {
    pub slug: &'static str,
    pub com1: PortIoDevice,
    pub legacy_bios_window: AddressRange,
    pub legacy_rsdp_window: AddressRange,
}

impl X86PlatformProfile {
    /// Validates port wiring and bounded firmware-window relationships.
    pub fn validate(self) -> Result<(), ValidationError> {
        if self.com1.base == 0 {
            return Err(ValidationError::ZeroPortBase);
        }
        if self.com1.irq >= 16 {
            return Err(ValidationError::InvalidIsaIrq);
        }
        if self.legacy_bios_window.size == 0 || self.legacy_rsdp_window.size == 0 {
            return Err(ValidationError::ZeroSizedFirmwareWindow);
        }
        let Some(bios_end) = self.legacy_bios_window.end() else {
            return Err(ValidationError::OverflowingFirmwareWindow);
        };
        let Some(rsdp_end) = self.legacy_rsdp_window.end() else {
            return Err(ValidationError::OverflowingFirmwareWindow);
        };
        if self.legacy_bios_window.base < self.legacy_rsdp_window.base || bios_end > rsdp_end {
            return Err(ValidationError::BiosWindowOutsideRsdpWindow);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Structural failures that make an x86 platform profile unsafe to consume.
pub enum ValidationError {
    ZeroPortBase,
    InvalidIsaIrq,
    ZeroSizedFirmwareWindow,
    OverflowingFirmwareWindow,
    BiosWindowOutsideRsdpWindow,
}

/// QEMU q35 platform facts used by the current x86_64 board descriptor.
pub const QEMU_Q35: X86PlatformProfile = X86PlatformProfile {
    slug: "qemu-q35-x86_64",
    com1: PortIoDevice {
        base: 0x03F8,
        irq: 4,
    },
    legacy_bios_window: AddressRange {
        base: 0x0008_0000,
        size: 0x0008_0000,
    },
    legacy_rsdp_window: AddressRange {
        base: 0,
        size: 0x0010_0000,
    },
};

#[cfg(test)]
mod tests;
