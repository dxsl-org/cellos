//! Minimal 16550 UART Driver for QEMU RISC-V Virt
//!
//! Used for kernel logging and early debug output.
//! Base Address: 0x10000000

use crate::sync::Spinlock;
use core::fmt;
use core::fmt::Write as _;

/// UART Registers (offset from base)
const _RHR: usize = 0; // Receive Holding Register (read)
const _THR: usize = 0; // Transmit Holding Register (write)
const IER: usize = 1; // Interrupt Enable Register
const FCR: usize = 2; // FIFO Control Register
const _ISR: usize = 2; // Interrupt Status Register
const LCR: usize = 3; // Line Control Register
const LSR: usize = 5; // Line Status Register

/// Line Status Flags
const _LSR_RX_READY: u8 = 1 << 0;
const _LSR_TX_EMPTY: u8 = 1 << 5;

#[allow(non_camel_case_types)]
pub struct viUART {
    base_addr: usize,
}

impl viUART {
    /// Create a new viUART instance (unsafe because base_addr must be valid)
    ///
    /// # Safety
    /// `base_addr` must be either 0 (sentinel meaning "no NS16550 MMIO on this
    /// board" — all I/O methods on this instance check for it and fall back to
    /// SBI DBCN/port-I/O) or a valid, identity-mapped 16550 UART MMIO base that
    /// remains mapped for the lifetime of the returned `viUART`.
    pub const unsafe fn new(base_addr: usize) -> Self {
        Self { base_addr }
    }

    /// Update the MMIO base address. Called once by `uart::init` from DTB info.
    pub fn set_base(&mut self, base: usize) {
        self.base_addr = base;
    }

    /// Initialize the UART
    pub fn init(&mut self) {
        if self.base_addr == 0 {
            // No NS16550 MMIO on this board (e.g. Pioneer SG2042 — UART at sv39-inaccessible
            // address). All console I/O goes via SBI DBCN; skip MMIO init entirely.
            return;
        }
        unsafe {
            let ptr = self.base_addr as *mut u8;

            // Disable interrupts
            ptr.add(IER).write_volatile(0x00);

            // Enable + clear FIFO (bit0=enable, bit1=clear RX, bit2=clear TX).
            ptr.add(FCR).write_volatile(0x07);

            // Set 8-bit mode (Word Length Select bits 0 and 1)
            ptr.add(LCR).write_volatile(0x03);

            // Keep UART RX interrupts DISABLED (IER=0): the console driver polls
            // the RHR directly. If RX IRQs were enabled, OpenSBI's M-mode console
            // handler could drain the RHR before the kernel's S-mode poll sees
            // the byte, swallowing all keyboard input.
            ptr.add(IER).write_volatile(0x00);
        }
    }

    // /// Write a single byte (Unused - Output via SBI)
    // pub fn write_byte(&mut self, byte: u8) { ... }
}

// impl fmt::Write for viUART { ... }

// Global Serial Instance protected by Spinlock
pub static SERIAL: Spinlock<viUART> = Spinlock::new(unsafe { viUART::new(0x10_000_000) });

// Direct writer to avoid stack buffering issues
struct DirectWriter;

impl fmt::Write for DirectWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for c in s.bytes() {
            #[cfg(any(target_arch = "riscv64", target_arch = "riscv32"))]
            {
                let _ = crate::hal::sbi::console_putchar(c);
            }
            #[cfg(all(target_arch = "aarch64", feature = "board-rpi3"))]
            {
                write_rpi3_console_byte(c);
            }
            #[cfg(all(target_arch = "aarch64", not(feature = "board-rpi3")))]
            {
                crate::hal::uart_pl011::putchar(c);
            }
            #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
            {
                crate::hal::uart_16550::putchar(c);
            }
        }
        Ok(())
    }
}

#[cfg(all(target_arch = "aarch64", feature = "board-rpi3"))]
fn write_rpi3_console_byte(byte: u8) {
    if byte == b'\n' {
        write_rpi3_console_byte(b'\r');
    }
    loop {
        // Synchronous exceptions mask IRQs while `sys_log` prints. Keep draining
        // the eight-symbol RX FIFO into the 4096-byte kernel ring so full-duplex
        // terminal traffic cannot overrun while TX waits for space.
        vi_handle_uart_irq();
        if crate::hal::uart_bcm_mini::try_putchar(byte) {
            break;
        }
        core::hint::spin_loop();
    }
}

/// Which writer owns the console line that is currently open.
///
/// The console is one UART shared by the kernel logger, every Cell and the
/// hypervisor's guest PL011 forwarding, and each of them writes records
/// independently. With a single "cursor is at a line start" flag, a record from
/// one writer landed *inside* the partial line of another: the guest shell
/// echoes one `sys_log` per keypress, so a diagnostic printed between two keys
/// shredded the command the operator was typing
/// (`USER: ping -c 3 192.16[net-loop] turns=215 …`). The owner is whichever
/// writer last started a line it has not terminated; a *different* writer that
/// begins a record closes that line first, so no writer can ever land inside
/// another writer's partial line. Consecutive records from the same writer (the
/// per-key echo, `print()` followed by its `println()`) stay inline as before.
#[derive(Clone, Copy)]
struct ConsoleLine {
    owner: usize,
}

/// No writer owns the line: the console is at the start of a fresh line.
const LINE_START: usize = usize::MAX;

/// Owner tag for kernel `log` records. No task id can hold it, so a kernel
/// record and a Cell record always see each other as foreign writers.
const KERNEL_LOG_OWNER: usize = usize::MAX - 1;

impl ConsoleLine {
    const fn new() -> Self {
        Self { owner: LINE_START }
    }

    /// Whether the console is between lines (nothing written since the last `\n`).
    fn at_line_start(&self) -> bool {
        self.owner == LINE_START
    }

    /// Claim the line for `writer`.
    ///
    /// Returns `(emit_break, owes_prefix)`: `emit_break` when the line was left
    /// open by a *different* writer and must be closed with a newline first, and
    /// `owes_prefix` when the record starts at a line start (either the line was
    /// already fresh, or the break just made it so) and therefore owes the
    /// per-line prefix.
    fn begin_record_state(&mut self, writer: usize) -> (bool, bool) {
        let at_start = self.at_line_start();
        let foreign = self.owner != LINE_START && self.owner != writer;
        self.owner = writer;
        (foreign, at_start || foreign)
    }

    /// Record the bytes just written: a trailing newline ends the line.
    fn wrote(&mut self, chunk: &str) {
        if chunk.ends_with('\n') {
            self.owner = LINE_START;
        }
    }
}

/// Console owner state, guarded together with the UART write so the owner a
/// record sees cannot change between its line-break decision and its first byte.
static CONSOLE: Spinlock<ConsoleLine> = Spinlock::new(ConsoleLine::new());

/// Emit one `print_user_log` record: the `USER: ` prefix at each line start.
///
/// The body is a free function over an `emit` sink so the host test drives the
/// exact loop an image drives (with a `String` in place of the UART), instead of
/// re-implementing the discipline beside it. `line` carries the owner across
/// records; `emit` receives the bytes in order.
fn emit_user_record(
    line: &mut ConsoleLine,
    writer: usize,
    msg: &str,
    mut emit: impl FnMut(&str),
) {
    let (broke, fresh) = line.begin_record_state(writer);
    if broke {
        emit("\n");
    }
    let mut at_start = fresh;
    let mut rest = msg;
    while !rest.is_empty() {
        if at_start {
            emit("USER: ");
            at_start = false;
        }
        match rest.find('\n') {
            Some(i) => {
                emit(&rest[..=i]);
                line.wrote(&rest[..=i]);
                at_start = true;
                rest = &rest[i + 1..];
            }
            None => {
                emit(rest);
                line.wrote(rest);
                rest = "";
            }
        }
    }
}

/// Write one Cell console record owned by `writer`, with the `USER: ` prefix.
///
/// USER stdout (cell `println`/`sys_log`) MUST always appear regardless of the
/// kernel's `log::max_level` — it is application output, not kernel debug chatter.
/// Routing it through `log::info!` (as `print_user_log` once did) meant lowering
/// the kernel log level to silence boot spam also silenced the shell prompt.
///
/// `writer` is the calling task's id, or [`KERNEL_LOG_OWNER`] for the kernel
/// logger. A record that would land inside another writer's open line first emits
/// the newline that closes it — one line, one writer.
pub fn write_user_record(writer: usize, msg: &str) {
    let mut line = CONSOLE.lock();
    emit_user_record(&mut line, writer, msg, |s| {
        let _ = DirectWriter.write_str(s);
    });
}

/// One kernel `log` record: holds the console for the whole record so its bytes
/// are emitted contiguously, and closes a foreign open line before it starts.
struct ConsoleRecord<'a> {
    state: crate::sync::SpinlockGuard<'a, ConsoleLine>,
}

impl ConsoleRecord<'_> {
    /// Append `s` to this record.
    pub fn write(&mut self, s: &str) {
        let _ = DirectWriter.write_str(s);
        self.state.wrote(s);
    }
}

impl fmt::Write for ConsoleRecord<'_> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.write(s);
        Ok(())
    }
}

/// Begin a console record owned by `writer` (the kernel logger's entry point).
fn begin_record(writer: usize) -> ConsoleRecord<'static> {
    let mut state = CONSOLE.lock();
    let (broke, _) = state.begin_record_state(writer);
    if broke {
        let _ = DirectWriter.write_str("\n");
    }
    ConsoleRecord { state }
}

// Logger integration
struct SimpleLogger;

impl log::Log for SimpleLogger {
    fn enabled(&self, _metadata: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            use fmt::Write;
            let mut out = begin_record(KERNEL_LOG_OWNER);
            let _ = writeln!(out, "[{:>5}] {}", record.level(), record.args());
        }
    }

    fn flush(&self) {}
}
static LOGGER: SimpleLogger = SimpleLogger;

pub fn init() {
    #[cfg(target_arch = "riscv64")]
    {
        // Read UART base from DTB (platform::init must have run first).
        let base = crate::platform::with(|p| p.uart_base);
        SERIAL.lock().set_base(base);
        SERIAL.lock().init();
    }
    // Register the log backend (works on all architectures; DirectWriter routes
    // to the correct UART per target_arch inside write_str).
    let _ = log::set_logger(&LOGGER).map(|()| log::set_max_level(log::LevelFilter::Info));
    // Logged here (not in platform::init) because the logger only just came up —
    // platform::init's own log line is emitted before set_logger and is lost.
    #[cfg(target_arch = "riscv64")]
    log::info!("[uart] RX/TX base = {:#x}", SERIAL.lock().base_addr);
}

// --- Input Handling ---

use alloc::collections::VecDeque;

// Global RX Buffer (Initialized late)
pub static RX_BUFFER: Spinlock<Option<VecDeque<u8>>> = Spinlock::new(None);
const MAX_RX_BUFFERED: usize = 4096;

/// Initialize Input Buffer (Must be called after Heap Init)
pub fn init_input() {
    *RX_BUFFER.lock() = Some(VecDeque::with_capacity(MAX_RX_BUFFERED));
    log::info!("UART Input Buffer Initialized");
}

/// Poll for a character from the IRQ-filled buffer.
pub fn getchar() -> Option<u8> {
    if let Some(buf) = RX_BUFFER.lock().as_mut() {
        return buf.pop_front();
    }
    None
}

/// Directly poll the 16550 Receive Holding Register.
///
/// This is the most robust input path on QEMU virt: it does not depend on
/// PLIC interrupt delegation to S-mode (which OpenSBI may keep in M-mode) nor
/// on the SBI DBCN console-read extension being implemented. Returns the byte
/// if LSR.DR (Data Ready, bit 0) is set, else `None`.
pub fn poll_rhr() -> Option<u8> {
    let base = SERIAL.lock().base_addr;
    if base == 0 {
        // No NS16550 MMIO (e.g. Pioneer SG2042). Fall back to SBI DBCN console read
        // so the interactive shell still receives keystrokes via the firmware console.
        #[cfg(target_arch = "riscv64")]
        {
            let c = crate::hal::sbi::console_getchar();
            return if c >= 0 { Some(c as u8) } else { None };
        }
        #[cfg(not(target_arch = "riscv64"))]
        return None;
    }
    // SAFETY: base_addr is the identity-mapped UART MMIO region, mapped in
    // init_kernel_paging. RHR (offset 0) is read-only; we gate on LSR.DR anyway.
    unsafe {
        let ptr = base as *mut u8;
        if (ptr.add(LSR).read_volatile() & _LSR_RX_READY) != 0 {
            Some(ptr.add(_RHR).read_volatile())
        } else {
            None
        }
    }
}

/// Called from the UART RX IRQ handler.
///
/// On RISC-V / AArch64: reads from the MMIO base stored in SERIAL.
/// On x86_64: delegates to the configured HAL port-I/O UART mechanism.
/// Handles CR→LF normalisation and pushes bytes into RX_BUFFER for the shell.
#[no_mangle]
pub extern "Rust" fn vi_handle_uart_irq() {
    if let Some(buf) = RX_BUFFER.lock().as_mut() {
        // Drain the UART receive FIFO; stop when no more data is ready.
        loop {
            // Read LSR (offset 5) to check Data Ready (bit 0).
            let (lsr, rhr_byte): (u8, Option<u8>) = {
                #[cfg(target_arch = "x86_64")]
                {
                    let byte = crate::hal::uart_16550::poll_input();
                    (byte.map_or(0, |_| _LSR_RX_READY), byte)
                }
                #[cfg(target_arch = "x86")]
                {
                    let lsr = unsafe {
                        let value: u8;
                        core::arch::asm!(
                            "in al, dx",
                            in("dx") (0x3F8u16 + LSR as u16),
                            out("al") value,
                            options(nomem, nostack)
                        );
                        value
                    };
                    let byte = if lsr & _LSR_RX_READY != 0 {
                        let value: u8;
                        unsafe {
                            core::arch::asm!(
                                "in al, dx",
                                in("dx") 0x3F8u16,
                                out("al") value,
                                options(nomem, nostack)
                            );
                        }
                        Some(value)
                    } else {
                        None
                    };
                    (lsr, byte)
                }
                #[cfg(all(target_arch = "aarch64", feature = "board-rpi3"))]
                {
                    let byte = crate::hal::uart_bcm_mini::poll_rx();
                    (byte.map_or(0, |_| _LSR_RX_READY), byte)
                }
                #[cfg(not(any(
                    target_arch = "x86_64",
                    target_arch = "x86",
                    all(target_arch = "aarch64", feature = "board-rpi3")
                )))]
                {
                    // MMIO path for RISC-V / AArch64.
                    let serial = SERIAL.lock();
                    let ptr = serial.base_addr as *mut u8;
                    // No MMIO UART on this board (e.g. Pioneer SG2042): bail out.
                    if ptr.is_null() {
                        break;
                    }
                    // SAFETY: MMIO region is identity-mapped and valid.
                    let lsr_val = unsafe { ptr.add(LSR).read_volatile() };
                    let byte = if lsr_val & _LSR_RX_READY != 0 {
                        Some(unsafe { ptr.add(_RHR).read_volatile() })
                    } else {
                        None
                    };
                    (lsr_val, byte)
                }
            };
            let _ = lsr;
            match rhr_byte {
                None => break,
                Some(c) => {
                    let c = if c == b'\r' { b'\n' } else { c };
                    if buf.len() < MAX_RX_BUFFERED {
                        buf.push_back(c);
                    }
                }
            }
        }
    }
}

#[cfg(any(target_arch = "aarch64", target_arch = "x86_64"))]
const _: crate::hal::HandleUartIrq = vi_handle_uart_irq;

#[cfg(test)]
mod tests {
    use super::{emit_user_record, ConsoleLine, KERNEL_LOG_OWNER};

    /// The bytes a real console would receive for one record, through the same
    /// loop an image runs — only the UART is replaced by a `String`.
    fn record(line: &mut ConsoleLine, writer: usize, msg: &str) -> alloc::string::String {
        let mut out = alloc::string::String::new();
        emit_user_record(line, writer, msg, |s| out.push_str(s));
        out
    }

    /// `print()` concatenation and the `USER: ` prefix: consecutive records from
    /// one writer stay on one line, and the prefix is paid once per line — a
    /// per-record prefix would render `help` as four `USER: h`/`e`/`l`/`p` lines.
    #[test]
    fn same_writer_stays_inline() {
        let mut line = ConsoleLine::new();
        assert_eq!(record(&mut line, 7, "h"), "USER: h");
        assert_eq!(record(&mut line, 7, "elp"), "elp");
        assert_eq!(record(&mut line, 7, "\n"), "\n");
        assert_eq!(record(&mut line, 7, "next"), "USER: next");
    }

    /// The board defect this discipline exists for: the guest echoes its command
    /// one byte per `sys_log` from the hypervisor, so a diagnostic from another
    /// cell used to be spliced into the middle of the line the operator was
    /// typing (`USER: ping -c 3 192.16[net-loop] turns=215 …`). A foreign writer
    /// now closes that line first instead of landing inside it.
    #[test]
    fn guest_echo_is_not_spliced_by_another_writer() {
        const GUEST: usize = 6;
        const NET: usize = 5;
        let mut line = ConsoleLine::new();
        assert_eq!(record(&mut line, GUEST, "ping -c 3 192.16"), "USER: ping -c 3 192.16");
        assert_eq!(
            record(&mut line, NET, "[net-loop] turns=215"),
            "\nUSER: [net-loop] turns=215"
        );
        assert_eq!(record(&mut line, NET, "\n"), "\n");
        // The guest's next byte starts a fresh line and re-owes the prefix: the
        // command is split across two lines but never absorbed into the
        // diagnostic's line.
        assert_eq!(record(&mut line, GUEST, "8.42.1"), "USER: 8.42.1");
    }

    /// A kernel `log` record is its own writer: it closes a Cell's open line (the
    /// break) and starts at a line start, and it never borrows the `USER: `
    /// prefix — the kernel logger formats through `ConsoleRecord`, which carries
    /// its own `[ INFO]` tag instead.
    #[test]
    fn kernel_record_closes_a_cell_line_first() {
        let mut line = ConsoleLine::new();
        assert_eq!(record(&mut line, 5, "Cellos > "), "USER: Cellos > ");
        assert_eq!(line.begin_record_state(KERNEL_LOG_OWNER), (true, true));
        line.wrote("[ INFO] x\n");
        // The kernel record ended its own line, so the Cell starts a fresh one.
        assert_eq!(record(&mut line, 5, "vfs"), "USER: vfs");
    }
}
