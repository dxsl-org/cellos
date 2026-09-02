use super::entry::EntryFrame;
use crate::x86_64::uart_16550::{putchar, puts};

fn hex(value: u64, digits: u32) {
    for shift in (0..digits).rev() {
        let nibble = ((value >> (shift * 4)) & 0xf) as u8;
        putchar(if nibble < 10 {
            b'0' + nibble
        } else {
            b'a' + nibble - 10
        });
    }
}

pub(super) fn halt(frame: &EntryFrame) -> ! {
    puts("[FAULT] x86 IDT vector=0x");
    hex(frame.vector, 2);
    puts(" error=0x");
    hex(frame.error, 16);
    puts(" rip=0x");
    hex(frame.rip, 16);
    puts(" cs=0x");
    hex(frame.cs, 4);
    putchar(b'\n');

    unsafe { core::arch::asm!("cli", options(nomem, nostack)) };
    loop {
        unsafe { core::arch::asm!("hlt", options(nomem, nostack)) };
    }
}
