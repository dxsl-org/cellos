// SPDX-License-Identifier: MPL-2.0
//! SPCR (Serial Port Console Redirection) parser.

use super::bytes::TableBytes;
use super::records::Gas;
use super::AcpiInfo;

/// SPCR facts (Microsoft SPCR specification, revisions 1-4).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SpcrInfo {
    pub revision: u8,
    /// 0x00 = 16550, 0x01 = 16450, 0x12 = 16550-compatible with GAS parameters.
    pub interface_type: u8,
    pub base: Gas,
    /// Bit 0 PC-AT IRQ, bit 1 I/O APIC, bit 2 SAPIC, bit 3 GIC.
    pub interrupt_type: u8,
    pub irq: u8,
    pub gsi: u32,
    /// 3 = 9600, 4 = 19200, 6 = 57600, 7 = 115200, 0 = as configured.
    pub baud_code: u8,
    /// `0xFFFF` when the port is not a PCI device.
    pub pci_device_id: u16,
    pub pci_vendor_id: u16,
    pub pci_bus: u8,
    pub pci_device: u8,
    pub pci_function: u8,
    pub pci_segment: u8,
    /// Revision 3 and later only; 0 = not specified.
    pub uart_clock_hz: u32,
}

impl SpcrInfo {
    /// The interface types a 16550-family driver can serve.
    pub fn is_16550_family(&self) -> bool {
        matches!(self.interface_type, 0x00 | 0x01 | 0x12)
    }
}

/// Parse the SPCR.
///
/// Layout after the SDT header:
///
/// | Offset | Field |
/// |---|---|
/// | 36 | interface type |
/// | 40 | base address GAS |
/// | 52 | interrupt type |
/// | 53 | IRQ |
/// | 54 | GSI |
/// | 58 | baud |
/// | 64 | PCI device ID |
/// | 66 | PCI vendor ID |
/// | 68-70 | PCI bus, device, function |
/// | 75 | PCI segment |
/// | 76 | UART clock (revision ≥ 3) |
pub(super) fn parse_spcr(virt: usize, length: usize, info: &mut AcpiInfo) {
    if length < 80 {
        log::warn!("[acpi] SPCR too short ({} bytes)", length);
        return;
    }
    // SAFETY: the caller validated and mapped `[virt, virt + length)`.
    let t = unsafe { TableBytes::new(virt, length) };
    let u8_at = |off| t.u8(off).unwrap_or(0);
    let revision = u8_at(8);
    let spcr = SpcrInfo {
        revision,
        interface_type: u8_at(36),
        base: t.gas(40).unwrap_or_default(),
        interrupt_type: u8_at(52),
        irq: u8_at(53),
        gsi: t.u32(54).unwrap_or(0),
        baud_code: u8_at(58),
        pci_device_id: t.u16(64).unwrap_or(0xFFFF),
        pci_vendor_id: t.u16(66).unwrap_or(0xFFFF),
        pci_bus: u8_at(68),
        pci_device: u8_at(69),
        pci_function: u8_at(70),
        pci_segment: u8_at(75),
        uart_clock_hz: if revision >= 3 {
            t.u32(76).unwrap_or(0)
        } else {
            0
        },
    };
    log::info!(
        "[acpi] SPCR: rev={} type={:#04x} space={} base={:#x} gsi={} baud_code={}",
        spcr.revision,
        spcr.interface_type,
        spcr.base.space_id,
        spcr.base.address,
        spcr.gsi,
        spcr.baud_code
    );
    info.spcr = Some(spcr);
}
