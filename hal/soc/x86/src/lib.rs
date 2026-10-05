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

/// How an RS485 transceiver takes its direction, as declared by the machine.
///
/// Neither variant is claimed as working by any board today: QEMU models no
/// DE/RE line, so the timing each mechanism implies can only be measured on
/// hardware (phase 07). The declaration exists so a machine with a transceiver
/// has one place to state it, and so a descriptor that states it wrongly is a
/// validation error rather than a silent assumption.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Rs485Direction {
    /// The transceiver derives direction from RTS, so software only has to keep
    /// RTS asserted for the whole transmit burst.
    RtsAuto,
    /// Software drives an explicit DE/RE line: port-I/O offset `offset` from the
    /// port base carries it.
    Gpio { offset: u16 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// RS485 facts for one declared serial port.
pub struct Rs485Port {
    /// Index into [`X86PlatformProfile::serial_ports`].
    pub port: u8,
    pub direction: Rs485Direction,
    /// Transmit-to-receive turnaround the transceiver requires, in nanoseconds.
    /// Zero is rejected: a transceiver that needs no turnaround does not exist,
    /// and a zero here would read as "timing not considered".
    pub turnaround_ns: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Immutable x86 machine facts consumed before ACPI discovery succeeds.
pub struct X86PlatformProfile {
    pub slug: &'static str,
    /// The console port. Always `serial_ports[0]`; kept as its own field because
    /// every console path (early boot, panic, log) must not index a list.
    pub com1: PortIoDevice,
    /// Every 16550 port this machine declares, console first. Ports the machine
    /// does not actually expose fail the 16550 probe and are refused by name
    /// rather than assumed present.
    pub serial_ports: &'static [PortIoDevice],
    /// RS485 transceiver facts, one entry per port that has one.
    pub rs485: &'static [Rs485Port],
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
        if self.serial_ports.is_empty() {
            return Err(ValidationError::NoSerialPorts);
        }
        if self.serial_ports[0] != self.com1 {
            return Err(ValidationError::ConsoleNotFirstSerialPort);
        }
        for (index, port) in self.serial_ports.iter().enumerate() {
            if port.base == 0 {
                return Err(ValidationError::ZeroPortBase);
            }
            if port.irq >= 16 {
                return Err(ValidationError::InvalidIsaIrq);
            }
            if self.serial_ports[..index].iter().any(|p| p.base == port.base) {
                return Err(ValidationError::DuplicateSerialPort);
            }
        }
        for rs485 in self.rs485 {
            if rs485.port as usize >= self.serial_ports.len() {
                return Err(ValidationError::Rs485PortOutOfRange);
            }
            if rs485.turnaround_ns == 0 {
                return Err(ValidationError::Rs485ZeroTurnaround);
            }
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

    /// The port at `index`, or `None` when the profile declares fewer ports.
    pub const fn serial_port(self, index: usize) -> Option<PortIoDevice> {
        if index < self.serial_ports.len() {
            Some(self.serial_ports[index])
        } else {
            None
        }
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
    /// A profile with no declared serial ports has no console.
    NoSerialPorts,
    /// The console must be the first declared port.
    ConsoleNotFirstSerialPort,
    /// Two declared ports share one base address.
    DuplicateSerialPort,
    /// An RS485 declaration names a port the profile does not declare.
    Rs485PortOutOfRange,
    /// An RS485 declaration omits the transceiver turnaround.
    Rs485ZeroTurnaround,
}

/// The four standard PC COM ports, console first.
///
/// Both x86 profiles declare them: the addresses are the legacy ISA layout, and
/// a machine that exposes fewer fails the probe on the missing ones (named, not
/// assumed). A machine with non-standard port addresses needs its own profile —
/// the HCL record for that machine, not a guess here.
const STANDARD_COM_PORTS: [PortIoDevice; 4] = [
    PortIoDevice {
        base: 0x03F8,
        irq: 4,
    },
    PortIoDevice {
        base: 0x02F8,
        irq: 3,
    },
    PortIoDevice {
        base: 0x03E8,
        irq: 4,
    },
    PortIoDevice {
        base: 0x02E8,
        irq: 3,
    },
];

/// QEMU q35 platform facts used by the current x86_64 board descriptor.
pub const QEMU_Q35: X86PlatformProfile = X86PlatformProfile {
    slug: "qemu-q35-x86_64",
    com1: PortIoDevice {
        base: 0x03F8,
        irq: 4,
    },
    serial_ports: &STANDARD_COM_PORTS,
    // The QEMU model has no transceiver: RS485 is expressible but declared
    // nowhere, which is exactly what "not claimed" has to look like.
    rs485: &[],
    legacy_bios_window: AddressRange {
        base: 0x0008_0000,
        size: 0x0008_0000,
    },
    legacy_rsdp_window: AddressRange {
        base: 0,
        size: 0x0010_0000,
    },
};

/// Generic x86_64 PC/server compatibility baseline.
///
/// Pins the console wiring a machine must expose to be listed in
/// `docs/hardware-compatibility-list.md` (R1: standard COM1 port address and
/// IRQ) together with the standard legacy firmware windows. A machine that
/// exposes a different console address is not covered by this profile — it needs
/// its own facts — and per-machine capture fields (BIOS version, exact
/// storage/NIC controller, Secure Boot state) live in that document, not here.
pub const GENERIC_X86_PC: X86PlatformProfile = X86PlatformProfile {
    slug: "x86_64-pc",
    com1: PortIoDevice {
        base: 0x03F8,
        irq: 4,
    },
    serial_ports: &STANDARD_COM_PORTS,
    // No transceiver is declared for the compatibility baseline: an industrial
    // machine that has one states its DE/RE facts in its own machine record.
    rs485: &[],
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
