// SPDX-License-Identifier: MPL-2.0
//! 32-bit x86.

pub mod pio {
    //! Port I/O.

    /// Read one byte from I/O port `port`.
    ///
    /// # Safety
    /// Port reads can have device side effects; the caller owns the device.
    pub(crate) unsafe fn inb(port: u16) -> u8 {
        let value: u8;
        unsafe {
            core::arch::asm!(
                "in al, dx",
                in("dx") port,
                out("al") value,
                options(nomem, nostack)
            );
        }
        value
    }
}
