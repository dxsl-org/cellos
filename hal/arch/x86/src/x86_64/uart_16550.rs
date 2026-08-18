//! 16550A UART mechanism via x86 port I/O.
//!
//! Two-phase init:
//!   1. `init()` — baud-rate, framing, FIFO setup (IRQs disabled; used at early boot).
//!   2. `init_input_irq()` — enable UART RX + redirect the configured ISA IRQ.
//!      Call this AFTER the IOAPIC (and LAPIC) are live.

use core::sync::atomic::{AtomicU32, Ordering};

static CONFIG: AtomicU32 = AtomicU32::new(0);

/// IDT vector allocated for COM1 RX interrupts.
pub const UART_VECTOR: u8 = 0x24;

/// Configure the platform-owned port and ISA IRQ before the first UART access.
///
/// `port_base` is the first 16550 port and `isa_irq` is its legacy ISA line.
/// Repeating the same configuration is harmless.
///
/// # Panics
///
/// Panics for a zero port, a non-ISA IRQ, or a conflicting second configuration.
pub fn configure(port_base: u16, isa_irq: u8) {
    assert!(port_base != 0, "x86 UART port base must be non-zero");
    assert!(isa_irq < 16, "x86 UART IRQ must be an ISA IRQ");

    let config = u32::from(port_base) | (u32::from(isa_irq) << 16);
    if CONFIG
        .compare_exchange(0, config, Ordering::Relaxed, Ordering::Relaxed)
        .is_err_and(|configured| configured != config)
    {
        panic!("x86 UART configured more than once");
    }
}

fn config() -> u32 {
    let config = CONFIG.load(Ordering::Relaxed);
    assert!(config != 0, "x86 UART used before platform configuration");
    config
}

fn port_base() -> u16 {
    config() as u16
}

fn isa_irq() -> u8 {
    (config() >> 16) as u8
}

#[inline]
fn outb(port: u16, val: u8) {
    // SAFETY: port I/O on the configured UART does not affect memory safety.
    unsafe {
        core::arch::asm!("out dx, al", in("dx") port, in("al") val, options(nomem, nostack));
    }
}
#[inline]
fn inb(port: u16) -> u8 {
    let val: u8;
    // SAFETY: reading port I/O does not affect memory safety.
    unsafe {
        core::arch::asm!("in al, dx", in("dx") port, out("al") val, options(nomem, nostack));
    }
    val
}

/// Initialise COM1 at 115200 8N1. IRQs intentionally left DISABLED here;
/// call `init_input_irq()` later to enable them once the IOAPIC/LAPIC are ready.
pub fn init() {
    let port = port_base();
    outb(port + 1, 0x00); // Disable IRQs
    outb(port + 3, 0x80); // DLAB = 1
    outb(port, 0x01); // Divisor low  (115200 baud)
    outb(port + 1, 0x00); // Divisor high
    outb(port + 3, 0x03); // 8N1
    outb(port + 2, 0xC7); // FIFO, 14-byte threshold
    outb(port + 4, 0x0B); // MCR: OUT2 (enables IOAPIC IRQ delivery) + RTS + DSR
}

/// Poll one COM1 byte without requiring ACPI, LAPIC, or IOAPIC routing.
///
/// This is the pre-ACPI receive diagnostic path. Physical IRQ delivery is a
/// separate sub-gate enabled by [`init_input_irq`] after MADT validation.
pub fn poll_input() -> Option<u8> {
    let port = port_base();
    if inb(port + 5) & 0x01 != 0 {
        Some(inb(port))
    } else {
        None
    }
}

/// Enable COM1 RX interrupts and route IOAPIC IRQ 4 → IDT vector 0x24.
///
/// Preconditions: `init()` called, LAPIC and IOAPIC are initialised
/// (i.e. after `crate::init_timers()` in kmain).
///
/// After this call, each received byte fires vector 0x24, which calls
/// `vi_handle_uart_irq()` → pushes the byte into the kernel RX buffer →
/// the shell's `sys_recv` on the input service drains it.
pub fn init_input_irq() {
    let port = port_base();
    // 1. Enable UART RX-ready interrupt (IER bit 0).
    outb(port + 1, 0x01);

    // 2. Wire the configured IOAPIC ISA IRQ to the UART vector on CPU 0.
    //    ioapic_redirect(irq, vec) sets: destination=CPU 0, edge-triggered, active-high.
    super::apic::ioapic_redirect(isa_irq(), UART_VECTOR);
}

/// Write one byte, blocking on TX hold register empty.
pub fn putchar(byte: u8) {
    let port = port_base();
    while inb(port + 5) & 0x20 == 0 {
        core::hint::spin_loop();
    }
    outb(port, byte);
}
/// Write string, converting `\n` to `\r\n`.
pub fn puts(s: &str) {
    for b in s.bytes() {
        if b == b'\n' {
            putchar(b'\r');
        }
        putchar(b);
    }
}
