//! 16550A UART mechanism via x86 port I/O.
//!
//! Two-phase init:
//!   1. `init()` — baud-rate, framing, FIFO setup (IRQs disabled; used at early boot).
//!   2. `init_input_irq()` — enable UART RX + redirect the configured ISA IRQ.
//!      Call this AFTER the IOAPIC (and LAPIC) are live.

use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

/// One declared serial port: port-I/O base plus its legacy ISA IRQ.
///
/// Mirrors the board profile's `PortIoDevice` without depending on it, so the
/// arch HAL stays free of board facts — the kernel converts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SerialPortSpec {
    pub base: u16,
    pub irq: u8,
}

/// Ports this mechanism can carry. A machine declaring more needs a bigger
/// table, not a silently dropped tail: `configure_all` refuses the excess.
pub const MAX_PORTS: usize = 8;

/// Registered ports, console first: `base | irq << 16`, 0 = absent/unused.
static PORTS: [AtomicU32; MAX_PORTS] = [const { AtomicU32::new(0) }; MAX_PORTS];
/// Usable port count, console included.
static PORT_COUNT: AtomicUsize = AtomicUsize::new(0);
/// Console slot: the pre-existing single-port configuration.
static CONFIG: AtomicU32 = AtomicU32::new(0);

/// IDT vector allocated for COM1 RX interrupts.
pub const UART_VECTOR: u8 = 0x24;

/// Configure the platform-owned console port and ISA IRQ before the first UART
/// access.
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

/// Register every declared port, probing all but the console.
///
/// Index 0 is the console: it is already carrying the log, so it is trusted and
/// takes the slot unconditionally. Every later port must pass [`probe`] before it
/// is offered, which is what turns "the descriptor declares a port the machine
/// does not have" into a named refusal instead of a cell writing into empty port
/// space.
///
/// Returns the number of usable ports, console included. A declared list longer
/// than [`MAX_PORTS`] is truncated and the excess is refused (never dropped
/// silently).
pub fn configure_all(ports: &[SerialPortSpec]) -> usize {
    let declared = ports.len().min(MAX_PORTS);
    let mut usable = 0usize;

    for (index, spec) in ports.iter().take(declared).enumerate() {
        if spec.base == 0 || spec.irq >= 16 {
            continue;
        }
        let console = index == 0;
        if !console && !probe(spec.base) {
            continue;
        }
        let packed = u32::from(spec.base) | (u32::from(spec.irq) << 16);
        PORTS[index].store(packed, Ordering::Relaxed);
        if !console {
            // Program the port before offering it, so a cell never sees a
            // half-configured UART.
            program_115200(spec.base, 0x03);
        }
        usable = index + 1;
    }

    PORT_COUNT.store(usable, Ordering::Relaxed);
    usable
}

/// Probe one port for a 16550 register set, restoring every register it touches.
///
/// Two independent tests, because a floating bus and a non-UART device fail them
/// differently:
///   * the scratch register (offset 7) must return what was written — a missing
///     port reads back 0xFF or the last value written;
///   * the divisor latch (DLAB set) must return what was written — an 8250-class
///     part without the latch, or unrelated hardware, will not.
///
/// The caller must not probe the console: it is already configured, and a probe
/// would perturb its divisor latch mid-log.
fn probe(base: u16) -> bool {
    let saved_scratch = inb(base + 7);
    outb(base + 7, 0xA5);
    let scratch = inb(base + 7);
    outb(base + 7, saved_scratch);
    if scratch != 0xA5 {
        return false;
    }

    let saved_lcr = inb(base + 3);
    outb(base + 3, 0x80); // DLAB
    let saved_dll = inb(base);
    let saved_dlm = inb(base + 1);
    outb(base, 0x5A);
    let latch = inb(base);
    outb(base, saved_dll);
    outb(base + 1, saved_dlm);
    outb(base + 3, saved_lcr);
    latch == 0x5A
}

/// Number of usable ports (console included) registered by [`configure_all`].
pub fn port_count() -> usize {
    PORT_COUNT.load(Ordering::Relaxed)
}

/// Declared base of a registered port, or `None` when the slot is empty.
pub fn port_base_at(index: usize) -> Option<u16> {
    let packed = PORTS.get(index)?.load(Ordering::Relaxed);
    if packed == 0 {
        None
    } else {
        Some(packed as u16)
    }
}

/// Declared ISA IRQ of a registered port, or `None` when the slot is empty.
pub fn port_irq_at(index: usize) -> Option<u8> {
    let packed = PORTS.get(index)?.load(Ordering::Relaxed);
    if packed == 0 {
        None
    } else {
        Some((packed >> 16) as u8)
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
    // MCR bit 3 (OUT2) gates interrupt delivery to the IOAPIC on the console.
    program_115200(port_base(), 0x0B);
}

/// Program one 16550 port at 115200 8N1 with IRQs disabled.
///
/// `mcr` carries the modem-control byte: the console sets OUT2 (`0x0B`) because
/// its IRQ is routed through the IOAPIC; the extra ports are polled and use
/// `0x03`.
fn program_115200(port: u16, mcr: u8) {
    outb(port + 1, 0x00); // Disable IRQs
    outb(port + 3, 0x80); // DLAB = 1
    outb(port, 0x01); // Divisor low  (115200 baud)
    outb(port + 1, 0x00); // Divisor high
    outb(port + 3, 0x03); // 8N1
    outb(port + 2, 0xC7); // FIFO, 14-byte threshold
    outb(port + 4, mcr);
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

/// Write one byte to a registered port, blocking on its TX hold register.
///
/// Returns `false` for an unregistered index, so a caller can never emit into
/// port space the machine did not prove it has.
pub fn write_to(index: usize, byte: u8) -> bool {
    let Some(port) = port_base_at(index) else {
        return false;
    };
    while inb(port + 5) & 0x20 == 0 {
        core::hint::spin_loop();
    }
    outb(port, byte);
    true
}

/// Poll one byte from a registered port without requiring IRQ routing.
///
/// The extra ports are polled rather than interrupt-driven: each port would need
/// its own IDT vector and an IIR-based demultiplexer, which this phase does not
/// add. Returns `None` for an unregistered index or an empty FIFO.
pub fn read_from(index: usize) -> Option<u8> {
    let port = port_base_at(index)?;
    if inb(port + 5) & 0x01 != 0 {
        Some(inb(port))
    } else {
        None
    }
}

/// Initialise a registered port at 115200 8N1 with IRQs disabled.
///
/// Used for the extra ports; the console keeps [`init`] + [`init_input_irq`].
pub fn init_port(index: usize) -> bool {
    let Some(port) = port_base_at(index) else {
        return false;
    };
    program_115200(port, 0x03);
    true
}
/// Set a registered port's baud rate, keeping 8N1 and IRQs disabled.
///
/// Supported rates are the exact standard divisors from a 1.8432 MHz clock:
/// 115200, 57600, 38400, 19200, 9600. Anything else is refused rather than
/// rounded, so a caller never gets a line speed it did not ask for.
pub fn configure_baud(index: usize, baud: u32) -> Result<(), ()> {
    let Some(port) = port_base_at(index) else {
        return Err(());
    };
    let divisor: u16 = match baud {
        115_200 => 1,
        57_600 => 2,
        38_400 => 3,
        19_200 => 6,
        9_600 => 12,
        _ => return Err(()),
    };
    let saved_lcr = inb(port + 3);
    outb(port + 3, 0x80); // DLAB
    outb(port, (divisor & 0xFF) as u8);
    outb(port + 1, (divisor >> 8) as u8);
    outb(port + 3, saved_lcr & 0x7F); // clear DLAB, keep the framing bits
    Ok(())
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
