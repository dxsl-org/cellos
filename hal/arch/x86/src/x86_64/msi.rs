//! MSI / MSI-X message encoding and the IDT vector range handed to PCIe functions.
//!
//! The kernel allocates vectors from [`VECTOR_FIRST`]`..=`[`VECTOR_LAST`] and
//! registers one handler; the IDT routes every vector in that range to it.
//! Without interrupt remapping the message carries an 8-bit destination APIC
//! ID, so a BSP with an APIC ID above 255 cannot be targeted.

use core::sync::atomic::{AtomicUsize, Ordering};

/// First IDT vector handed to MSI/MSI-X. Below it: exceptions, the LAPIC timer
/// (0x20) and IOAPIC-routed legacy lines (0x21..=0x2F).
pub const VECTOR_FIRST: u8 = 0x30;
/// Last IDT vector handed to MSI/MSI-X. 0xE0..=0xEF stay free; 0xF0 is the IPI.
pub const VECTOR_LAST: u8 = 0xDF;
/// The legacy `int 0x80` gate sits inside the range and is never handed out.
const LEGACY_INT80: u8 = 0x80;

/// Fixed-delivery, physical-destination MSI address window.
const MSI_ADDRESS_BASE: u64 = 0xFEE0_0000;

/// Whether `vector` belongs to the MSI range (and is therefore routed to the
/// registered handler).
pub const fn is_msi_vector(vector: u8) -> bool {
    vector >= VECTOR_FIRST && vector <= VECTOR_LAST && vector != LEGACY_INT80
}

/// MSI address/data for an edge-triggered, fixed-delivery interrupt on
/// `vector`, physically addressed to `dest_apic_id`. `None` when the ID does
/// not fit the 8-bit destination field or the vector is outside the range.
pub const fn encode(dest_apic_id: u32, vector: u8) -> Option<(u64, u32)> {
    if dest_apic_id > 0xFF || !is_msi_vector(vector) {
        return None;
    }
    Some((
        MSI_ADDRESS_BASE | ((dest_apic_id as u64) << 12),
        vector as u32,
    ))
}

/// The message that targets the bootstrap processor.
pub fn bsp_message(vector: u8) -> Option<(u64, u32)> {
    encode(super::apic::cpu_apic_id(0)?, vector)
}

static HANDLER: AtomicUsize = AtomicUsize::new(0);

/// Install the kernel's MSI handler. The IDT drops (EOIs) MSI vectors until a
/// handler is set. The handler runs in interrupt context.
pub fn set_handler(handler: fn(u8)) {
    HANDLER.store(handler as usize, Ordering::Release);
}

/// Deliver `vector` to the registered handler; called by the IDT dispatcher.
pub(crate) fn deliver(vector: u8) {
    let raw = HANDLER.load(Ordering::Acquire);
    if raw != 0 {
        // SAFETY: `raw` was stored from a `fn(u8)` by `set_handler`.
        let handler: fn(u8) = unsafe { core::mem::transmute::<usize, fn(u8)>(raw) };
        handler(vector);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_excludes_reserved_vectors() {
        assert!(!is_msi_vector(0x20));
        assert!(!is_msi_vector(0x2F));
        assert!(is_msi_vector(0x30));
        assert!(!is_msi_vector(0x80));
        assert!(is_msi_vector(0xDF));
        assert!(!is_msi_vector(0xE0));
        assert!(!is_msi_vector(0xF0));
    }

    #[test]
    fn message_targets_destination_with_vector_data() {
        assert_eq!(encode(0, 0x30), Some((0xFEE0_0000, 0x30)));
        assert_eq!(encode(0x12, 0x41), Some((0xFEE1_2000, 0x41)));
        assert_eq!(encode(0x100, 0x41), None);
        assert_eq!(encode(1, 0x80), None);
    }
}
