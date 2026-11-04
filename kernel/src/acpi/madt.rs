// SPDX-License-Identifier: MPL-2.0
//! MADT (APIC) parser.

use super::bytes::TableBytes;
use super::records::{AcpiTopology, CpuEntry, IoApicEntry, LapicNmi};
use super::AcpiInfo;

/// Parse the MADT (Multiple APIC Description Table).
///
/// Legacy `AcpiInfo` fields keep their established meaning:
/// - `lapic_base`: header address, replaced by a type-5 override;
/// - `ioapic_base` / `ioapic_gsi_base`: the **last** type-1 record;
/// - `irq_overrides`: ISA IRQ → GSI from type 2.
///
/// Everything else goes to `topo`, in table order:
/// - processors from types 0 (xAPIC) and 9 (x2APIC);
/// - every I/O APIC;
/// - local NMIs from types 4 and 0xA;
/// - override flags.
///
/// Layout: local APIC address (u32) at 36, flags (u32) at 40, records from 44.
/// Each record is `type u8, length u8, payload`. A record whose length is
/// below 2 or runs past the table stops the walk but keeps what was read.
pub(super) fn parse_madt(virt: usize, length: usize, info: &mut AcpiInfo, topo: &mut AcpiTopology) {
    if length < 44 {
        log::warn!("[acpi] MADT too short ({} bytes)", length);
        return;
    }
    // SAFETY: the caller validated and mapped `[virt, virt + length)`.
    let table = unsafe { TableBytes::new(virt, length) };

    if let Some(lapic) = table.u32(36).filter(|addr| *addr != 0) {
        info.lapic_base = lapic as u64;
    }

    let mut offset = 44usize;
    while let (Some(kind), Some(len)) = (table.u8(offset), table.u8(offset + 1)) {
        let len = len as usize;
        if len < 2 || offset + len > table.len() {
            log::warn!("[acpi] MADT entry type={} has bad length={}", kind, len);
            break;
        }
        let at = |field: usize| offset + field;
        match kind {
            // Processor Local APIC: uid u8 @2, apic_id u8 @3, flags u32 @4.
            0 if len >= 8 => push_cpu(
                topo,
                CpuEntry {
                    acpi_uid: table.u8(at(2)).unwrap_or(0) as u32,
                    apic_id: table.u8(at(3)).unwrap_or(0) as u32,
                    flags: table.u32(at(4)).unwrap_or(0),
                    x2apic: false,
                },
            ),
            // I/O APIC: id u8 @2, address u32 @4, gsi_base u32 @8.
            1 if len >= 12 => {
                let address = table.u32(at(4)).unwrap_or(0);
                let gsi_base = table.u32(at(8)).unwrap_or(0);
                if address != 0 {
                    info.ioapic_base = address as u64;
                    info.ioapic_gsi_base = gsi_base;
                    topo.ioapics.push(IoApicEntry {
                        id: table.u8(at(2)).unwrap_or(0),
                        address: address as u64,
                        gsi_base,
                    });
                }
            }
            // Interrupt Source Override: bus u8 @2, source u8 @3, gsi u32 @4, flags u16 @8.
            2 if len >= 10 => {
                let bus = table.u8(at(2)).unwrap_or(1);
                let source = table.u8(at(3)).unwrap_or(u8::MAX) as usize;
                if bus == 0 && source < 16 {
                    info.irq_overrides[source] = table.u32(at(4)).unwrap_or(source as u32);
                    info.irq_override_flags[source] = table.u16(at(8)).unwrap_or(0);
                }
            }
            // Local APIC NMI: uid u8 @2, flags u16 @3, lint u8 @5.
            4 if len >= 6 => {
                let uid = table.u8(at(2)).unwrap_or(0);
                topo.lapic_nmis.push(LapicNmi {
                    acpi_uid: if uid == 0xFF { u32::MAX } else { uid as u32 },
                    flags: table.u16(at(3)).unwrap_or(0),
                    lint: table.u8(at(5)).unwrap_or(0),
                });
            }
            // Local APIC address override: address u64 @4.
            5 if len >= 12 => {
                if let Some(lapic) = table.u64(at(4)).filter(|addr| *addr != 0) {
                    info.lapic_base = lapic;
                }
            }
            // Processor Local x2APIC: x2apic_id u32 @4, flags u32 @8, uid u32 @12.
            9 if len >= 16 => push_cpu(
                topo,
                CpuEntry {
                    apic_id: table.u32(at(4)).unwrap_or(0),
                    flags: table.u32(at(8)).unwrap_or(0),
                    acpi_uid: table.u32(at(12)).unwrap_or(0),
                    x2apic: true,
                },
            ),
            // Local x2APIC NMI: flags u16 @2, uid u32 @4, lint u8 @8.
            0xA if len >= 12 => {
                topo.lapic_nmis.push(LapicNmi {
                    flags: table.u16(at(2)).unwrap_or(0),
                    acpi_uid: table.u32(at(4)).unwrap_or(0),
                    lint: table.u8(at(8)).unwrap_or(0),
                });
            }
            _ => {}
        }
        offset += len;
    }
}

/// Record a processor, keeping disabled-and-not-online-capable entries out:
/// firmware lists unpopulated sockets that way, and they are never startable.
fn push_cpu(topo: &mut AcpiTopology, cpu: CpuEntry) {
    if cpu.usable() {
        topo.cpus.push(cpu);
    }
}
