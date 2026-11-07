//! 16550A UART mechanism via x86 port I/O and MMIO.
//!
//! Two-phase init:
//!   1. `init()` — baud-rate, framing, FIFO setup (IRQs disabled; used at early boot).
//!   2. `init_input_irq()` — enable UART RX + redirect the configured ISA IRQ or GSI.
//!      Call this AFTER the IOAPIC (and LAPIC) are live.

use core::sync::atomic::{
    AtomicBool, AtomicU16, AtomicU32, AtomicU64, AtomicU8, AtomicUsize, Ordering,
};

static TX_LOCK: AtomicBool = AtomicBool::new(false);

struct UartLockGuard {
    flags: usize,
}

fn lock_uart() -> UartLockGuard {
    let flags: usize;
    // SAFETY: reading and masking local CPU flags prevents local interrupt preemptions.
    unsafe {
        core::arch::asm!("pushfq", "pop {}", "cli", out(reg) flags, options(nomem, nostack));
    }
    let mut spins = 0usize;
    while TX_LOCK
        .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        spins += 1;
        if spins > 1_000_000 {
            // Break lock on panic/NMI to avoid deadlocking diagnostic output.
            TX_LOCK.store(true, Ordering::Release);
            break;
        }
        core::hint::spin_loop();
    }
    UartLockGuard { flags }
}

impl Drop for UartLockGuard {
    fn drop(&mut self) {
        TX_LOCK.store(false, Ordering::Release);
        if self.flags & (1 << 9) != 0 {
            // SAFETY: restoring IF bit if it was set before acquiring lock.
            unsafe {
                core::arch::asm!("sti", options(nomem, nostack));
            }
        }
    }
}

/// Addressing for a 16550 UART controller (legacy I/O port or MMIO).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UartAddress {
    IoPort(u16),
    Mmio {
        base_phys: u64,
        base_virt: usize,
        stride: u8,
    },
}

/// Complete configuration for a console UART.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConsoleConfig {
    pub addr: UartAddress,
    pub irq: u8,
    pub gsi: u32,
    pub is_gsi: bool,
    pub baud: u32,
}

pub const DEFAULT_CONSOLE_CONFIG: ConsoleConfig = ConsoleConfig {
    addr: UartAddress::IoPort(0x03F8),
    irq: 4,
    gsi: 4,
    is_gsi: false,
    baud: 115200,
};

static CONSOLE_PORT: AtomicU16 = AtomicU16::new(0x03F8);
static CONSOLE_MMIO_PHYS: AtomicU64 = AtomicU64::new(0);
static CONSOLE_MMIO_VIRT: AtomicUsize = AtomicUsize::new(0);
static CONSOLE_MMIO_STRIDE: AtomicU8 = AtomicU8::new(1);
static CONSOLE_IS_MMIO: AtomicBool = AtomicBool::new(false);
static CONSOLE_GSI: AtomicU32 = AtomicU32::new(4);
static CONSOLE_IS_GSI: AtomicBool = AtomicBool::new(false);
static CONSOLE_BAUD: AtomicU32 = AtomicU32::new(115200);

/// One declared serial port: port-I/O base plus its legacy ISA IRQ.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SerialPortSpec {
    pub base: u16,
    pub irq: u8,
}

/// Ports this mechanism can carry.
pub const MAX_PORTS: usize = 8;

/// Registered ports, console first: `base | irq << 16`, 0 = absent/unused.
static PORTS: [AtomicU32; MAX_PORTS] = [const { AtomicU32::new(0) }; MAX_PORTS];
/// Usable port count, console included.
static PORT_COUNT: AtomicUsize = AtomicUsize::new(0);
/// Console slot: the pre-existing single-port configuration.
static CONFIG: AtomicU32 = AtomicU32::new(0);

/// IDT vector allocated for COM1/console RX interrupts.
pub const UART_VECTOR: u8 = 0x24;

/// Configure the platform-owned console port and ISA IRQ before the first UART access.
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
    set_console_config(ConsoleConfig {
        addr: UartAddress::IoPort(port_base),
        irq: isa_irq,
        gsi: isa_irq as u32,
        is_gsi: false,
        baud: 115200,
    });
}

/// Reconfigure the active console to a new port/MMIO and GSI/IRQ.
pub fn set_console_config(cfg: ConsoleConfig) {
    match cfg.addr {
        UartAddress::IoPort(port) => {
            CONSOLE_PORT.store(port, Ordering::Release);
            CONSOLE_IS_MMIO.store(false, Ordering::Release);
        }
        UartAddress::Mmio {
            base_phys,
            base_virt,
            stride,
        } => {
            CONSOLE_MMIO_PHYS.store(base_phys, Ordering::Release);
            CONSOLE_MMIO_VIRT.store(base_virt, Ordering::Release);
            CONSOLE_MMIO_STRIDE.store(stride, Ordering::Release);
            CONSOLE_IS_MMIO.store(true, Ordering::Release);
        }
    }
    CONSOLE_GSI.store(cfg.gsi, Ordering::Release);
    CONSOLE_IS_GSI.store(cfg.is_gsi, Ordering::Release);
    CONSOLE_BAUD.store(cfg.baud, Ordering::Release);
    program_uart(cfg.addr, cfg.baud, 0x0B);
}

/// Query the currently active console configuration.
pub fn active_console_config() -> ConsoleConfig {
    let is_mmio = CONSOLE_IS_MMIO.load(Ordering::Acquire);
    let addr = if is_mmio {
        UartAddress::Mmio {
            base_phys: CONSOLE_MMIO_PHYS.load(Ordering::Acquire),
            base_virt: CONSOLE_MMIO_VIRT.load(Ordering::Acquire),
            stride: CONSOLE_MMIO_STRIDE.load(Ordering::Acquire),
        }
    } else {
        UartAddress::IoPort(CONSOLE_PORT.load(Ordering::Acquire))
    };
    ConsoleConfig {
        addr,
        irq: (CONFIG.load(Ordering::Acquire) >> 16) as u8,
        gsi: CONSOLE_GSI.load(Ordering::Acquire),
        is_gsi: CONSOLE_IS_GSI.load(Ordering::Acquire),
        baud: CONSOLE_BAUD.load(Ordering::Acquire),
    }
}

/// Register every declared port, probing all but the console.
pub fn configure_all(ports: &[SerialPortSpec]) -> usize {
    let declared = ports.len().min(MAX_PORTS);
    let mut usable = 0usize;

    for (index, spec) in ports.iter().take(declared).enumerate() {
        if spec.base == 0 || spec.irq >= 16 {
            continue;
        }
        let console = index == 0;
        if !console && !probe_uart(UartAddress::IoPort(spec.base)) {
            continue;
        }
        let packed = u32::from(spec.base) | (u32::from(spec.irq) << 16);
        PORTS[index].store(packed, Ordering::Relaxed);
        if !console {
            program_115200(spec.base, 0x03);
        }
        usable = index + 1;
    }

    PORT_COUNT.store(usable, Ordering::Relaxed);
    usable
}

pub fn probe(base: u16) -> bool {
    probe_uart(UartAddress::IoPort(base))
}

/// Probe a 16550 UART controller at the given address.
pub fn probe_uart(addr: UartAddress) -> bool {
    let saved_scratch = reg_read(addr, 7);
    reg_write(addr, 7, 0xA5);
    let scratch = reg_read(addr, 7);
    reg_write(addr, 7, saved_scratch);
    if scratch != 0xA5 {
        return false;
    }

    let saved_lcr = reg_read(addr, 3);
    reg_write(addr, 3, 0x80); // DLAB
    let saved_dll = reg_read(addr, 0);
    let saved_dlm = reg_read(addr, 1);
    reg_write(addr, 0, 0x5A);
    let latch = reg_read(addr, 0);
    reg_write(addr, 0, saved_dll);
    reg_write(addr, 1, saved_dlm);
    reg_write(addr, 3, saved_lcr);
    latch == 0x5A
}

pub fn port_count() -> usize {
    PORT_COUNT.load(Ordering::Relaxed)
}

pub fn port_base_at(index: usize) -> Option<u16> {
    let packed = PORTS.get(index)?.load(Ordering::Relaxed);
    if packed == 0 {
        None
    } else {
        Some(packed as u16)
    }
}

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

pub fn port_base() -> u16 {
    config() as u16
}

fn isa_irq() -> u8 {
    (config() >> 16) as u8
}

#[inline]
fn outb(port: u16, val: u8) {
    unsafe {
        core::arch::asm!("out dx, al", in("dx") port, in("al") val, options(nomem, nostack));
    }
}

#[inline]
fn inb(port: u16) -> u8 {
    let val: u8;
    unsafe {
        core::arch::asm!("in al, dx", in("dx") port, out("al") val, options(nomem, nostack));
    }
    val
}

#[inline]
pub fn reg_read(addr: UartAddress, reg: u8) -> u8 {
    match addr {
        UartAddress::IoPort(port) => inb(port + reg as u16),
        UartAddress::Mmio {
            base_virt, stride, ..
        } => {
            let offset = (reg as usize) * (stride as usize);
            let ptr = (base_virt + offset) as *const u8;
            unsafe { core::ptr::read_volatile(ptr) }
        }
    }
}

#[inline]
pub fn reg_write(addr: UartAddress, reg: u8, val: u8) {
    match addr {
        UartAddress::IoPort(port) => outb(port + reg as u16, val),
        UartAddress::Mmio {
            base_virt, stride, ..
        } => {
            let offset = (reg as usize) * (stride as usize);
            let ptr = (base_virt + offset) as *mut u8;
            unsafe { core::ptr::write_volatile(ptr, val) }
        }
    }
}

#[inline]
pub fn console_addr() -> UartAddress {
    if CONSOLE_IS_MMIO.load(Ordering::Relaxed) {
        UartAddress::Mmio {
            base_phys: CONSOLE_MMIO_PHYS.load(Ordering::Relaxed),
            base_virt: CONSOLE_MMIO_VIRT.load(Ordering::Relaxed),
            stride: CONSOLE_MMIO_STRIDE.load(Ordering::Relaxed),
        }
    } else {
        UartAddress::IoPort(CONSOLE_PORT.load(Ordering::Relaxed))
    }
}

pub fn baud_divisor(baud: u32) -> u16 {
    match baud {
        9600 => 12,
        19200 => 6,
        38400 => 3,
        57600 => 2,
        _ => 1,
    }
}

pub fn program_uart(addr: UartAddress, baud: u32, mcr: u8) {
    let divisor = baud_divisor(baud);
    reg_write(addr, 1, 0x00); // Disable IRQs
    reg_write(addr, 3, 0x80); // DLAB = 1
    reg_write(addr, 0, (divisor & 0xFF) as u8);
    reg_write(addr, 1, (divisor >> 8) as u8);
    reg_write(addr, 3, 0x03); // 8N1, clear DLAB
    reg_write(addr, 2, 0xC7); // FIFO, 14-byte threshold
    reg_write(addr, 4, mcr);
}

pub fn init() {
    program_uart(console_addr(), CONSOLE_BAUD.load(Ordering::Relaxed), 0x0B);
}

fn program_115200(port: u16, mcr: u8) {
    program_uart(UartAddress::IoPort(port), 115200, mcr);
}

pub fn poll_input() -> Option<u8> {
    let addr = console_addr();
    if reg_read(addr, 5) & 0x01 != 0 {
        Some(reg_read(addr, 0))
    } else {
        None
    }
}

pub fn init_input_irq() -> bool {
    let is_gsi = CONSOLE_IS_GSI.load(Ordering::Acquire);
    let gsi = CONSOLE_GSI.load(Ordering::Acquire);
    let routed = if is_gsi {
        super::apic::ioapic_redirect_gsi(gsi, UART_VECTOR, 0)
    } else {
        super::apic::ioapic_redirect(isa_irq(), UART_VECTOR)
    };
    if !routed {
        return false;
    }
    let addr = console_addr();
    reg_write(addr, 1, 0x01);
    true
}

pub fn putchar(byte: u8) {
    let _guard = lock_uart();
    let addr = console_addr();
    while reg_read(addr, 5) & 0x20 == 0 {
        core::hint::spin_loop();
    }
    reg_write(addr, 0, byte);
}

pub fn write_to(index: usize, byte: u8) -> bool {
    let Some(port) = port_base_at(index) else {
        return false;
    };
    let _guard = lock_uart();
    let addr = UartAddress::IoPort(port);
    while reg_read(addr, 5) & 0x20 == 0 {
        core::hint::spin_loop();
    }
    reg_write(addr, 0, byte);
    true
}

pub fn read_from(index: usize) -> Option<u8> {
    let port = port_base_at(index)?;
    let addr = UartAddress::IoPort(port);
    if reg_read(addr, 5) & 0x01 != 0 {
        Some(reg_read(addr, 0))
    } else {
        None
    }
}

pub fn init_port(index: usize) -> bool {
    let Some(port) = port_base_at(index) else {
        return false;
    };
    program_115200(port, 0x03);
    true
}

pub fn configure_baud(index: usize, baud: u32) -> bool {
    let Some(port) = port_base_at(index) else {
        return false;
    };
    program_uart(UartAddress::IoPort(port), baud, 0x03);
    true
}

pub fn puts(s: &str) {
    let _guard = lock_uart();
    let addr = console_addr();
    for b in s.bytes() {
        if b == b'\n' {
            while reg_read(addr, 5) & 0x20 == 0 {
                core::hint::spin_loop();
            }
            reg_write(addr, 0, b'\r');
        }
        while reg_read(addr, 5) & 0x20 == 0 {
            core::hint::spin_loop();
        }
        reg_write(addr, 0, b);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_addressing_mmio_strides() {
        let mut buffer = [0u8; 64];
        let base_virt = buffer.as_mut_ptr() as usize;

        // Stride 1: reg 5 is at offset 5
        let addr_stride1 = UartAddress::Mmio {
            base_phys: 0x1000,
            base_virt,
            stride: 1,
        };
        reg_write(addr_stride1, 5, 0x20);
        assert_eq!(buffer[5], 0x20);
        assert_eq!(reg_read(addr_stride1, 5), 0x20);

        // Stride 4: reg 5 is at offset 20
        buffer.fill(0);
        let addr_stride4 = UartAddress::Mmio {
            base_phys: 0x1000,
            base_virt,
            stride: 4,
        };
        reg_write(addr_stride4, 5, 0x20);
        assert_eq!(buffer[20], 0x20);
        assert_eq!(reg_read(addr_stride4, 5), 0x20);
    }

    #[test]
    fn baud_divisor_values() {
        assert_eq!(baud_divisor(9600), 12);
        assert_eq!(baud_divisor(19200), 6);
        assert_eq!(baud_divisor(38400), 3);
        assert_eq!(baud_divisor(57600), 2);
        assert_eq!(baud_divisor(115200), 1);
    }

    #[test]
    fn probe_on_mock_mmio_buffer() {
        let mut buffer = [0u8; 64];
        let base_virt = buffer.as_mut_ptr() as usize;
        let addr = UartAddress::Mmio {
            base_phys: 0x1000,
            base_virt,
            stride: 4,
        };
        reg_write(addr, 7, 0xA5);
        reg_write(addr, 0, 0x5A);
        assert!(probe_uart(addr));
    }
}
