//! Allocation-free UART output for boot stages where the logger is not live.
//!
//! `log::info!` is dropped before the logger is installed, and some boot stages
//! run long before that; a decision made there still has to be observable, or it
//! cannot be witnessed (see `paging::activate_paging`). Nothing here allocates:
//! the heap may not exist yet.

/// Write one byte to the early console.
pub(crate) fn put(byte: u8) {
    crate::hal::uart_16550::putchar(byte);
}

pub(crate) fn text(value: &str) {
    for byte in value.bytes() {
        put(byte);
    }
}

pub(crate) fn flag(value: bool) {
    text(if value { "true" } else { "false" });
}

/// Lowercase hex with `0x`, no leading zeros (except for zero itself).
pub(crate) fn hex(value: usize) {
    text("0x");
    let mut started = false;
    for nibble_index in (0..(core::mem::size_of::<usize>() * 2)).rev() {
        let nibble = ((value >> (nibble_index * 4)) & 0xf) as u8;
        if nibble != 0 {
            started = true;
        }
        if started || nibble_index == 0 {
            put(if nibble < 10 {
                b'0' + nibble
            } else {
                b'a' + nibble - 10
            });
        }
    }
}
