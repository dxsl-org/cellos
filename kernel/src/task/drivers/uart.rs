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
/// writer emitted the last bytes of a line it has not terminated; a *different*
/// writer that writes closes that line first, so no writer can ever land inside
/// another writer's partial line. Consecutive records from the same writer (the
/// per-key echo, `print()` followed by its `println()`, a long write split into
/// several records) stay inline.
///
/// The owner is derived from the bytes alone, per fragment: a fragment ending in
/// `\n` leaves the console at a line start, any other non-empty fragment leaves
/// its writer owning an open line. A record's unterminated tail after an inner
/// newline therefore owns the line it opened, and a record that writes nothing
/// changes nothing.
///
/// Every physical line carries exactly one tag at its start: `USER: ` for a Cell
/// line, `[LEVEL]` for a kernel line. When a kernel record interrupts an open
/// Cell line, that Cell line is *suspended*: the kernel line starts on its own
/// line, and the suspended writer's next bytes start a line tagged `USER+ `
/// ([`USER_CONTINUATION`]) instead of `USER: `. A `USER+ ` line is the exact
/// continuation of the last `USER: `/`USER+ ` line before the kernel lines
/// between them — the forced line break is not part of the Cell's output — so a
/// parser rebuilds the Cell's bytes by dropping kernel lines and joining each
/// `USER+ ` line onto the preceding Cell line. Any other Cell writer starting a
/// line ends the suspension (its line would sit between the two halves), and the
/// suspended writer then resumes with a plain `USER: `: a split by another Cell
/// is a real, unmarked line break.
#[derive(Clone, Copy)]
struct ConsoleLine {
    owner: usize,
    /// The Cell writer whose open line a kernel record closed, until it resumes.
    suspended: Option<usize>,
}

/// No writer owns the line: the console is at the start of a fresh line.
const LINE_START: usize = usize::MAX;

/// Owner tag for kernel `log` records. No task id can hold it, so a kernel
/// record and a Cell record always see each other as foreign writers.
const KERNEL_LOG_OWNER: usize = usize::MAX - 1;

/// Per-line tag of Cell console output; kernel records carry their own tag.
const USER_PREFIX: &str = "USER: ";

/// Tag of a Cell line that continues the Cell line a kernel record interrupted.
const USER_CONTINUATION: &str = "USER+ ";

impl ConsoleLine {
    const fn new() -> Self {
        Self { owner: LINE_START, suspended: None }
    }
}

/// Console owner state, guarded together with the UART write so the owner a
/// record sees cannot change between its line-break decision and its first byte.
static CONSOLE: Spinlock<ConsoleLine> = Spinlock::new(ConsoleLine::new());

/// Line tags of one writer: `prefix` starts each line it begins, `continuation`
/// starts the line resuming its own line that a kernel record interrupted.
struct Tags {
    prefix: &'static str,
    continuation: &'static str,
}

const USER_TAGS: Tags = Tags { prefix: USER_PREFIX, continuation: USER_CONTINUATION };
/// Kernel records format their own `[LEVEL]` tag.
const KERNEL_TAGS: Tags = Tags { prefix: "", continuation: "" };

/// Emit `msg` as console bytes owned by `writer`, tagging every physical line it
/// begins with `tags`.
///
/// Per line fragment: when the open line belongs to another writer it is closed
/// with `\n` first (a kernel writer closing a Cell line suspends that Cell, see
/// [`ConsoleLine`]); when the fragment starts a line (fresh, or just closed) it
/// gets the prefix, or the continuation tag when it resumes `writer`'s suspended
/// line; a fragment continuing `writer`'s own open line gets neither. Cell
/// records and kernel `log` records both run through here.
///
/// The body is a free function over an `emit` sink so the host test drives the
/// exact loop an image drives (with a `String` in place of the UART), instead of
/// re-implementing the discipline beside it. `line` carries the state across
/// records; `emit` receives the bytes in order.
fn emit_record(
    line: &mut ConsoleLine,
    writer: usize,
    tags: &Tags,
    msg: &str,
    mut emit: impl FnMut(&str),
) {
    let kernel = writer == KERNEL_LOG_OWNER;
    let mut rest = msg;
    while !rest.is_empty() {
        let end = rest.find('\n').map_or(rest.len(), |i| i + 1);
        let (fragment, tail) = rest.split_at(end);
        if line.owner != writer {
            if line.owner != LINE_START {
                emit("\n");
                if kernel {
                    line.suspended = Some(line.owner);
                }
            }
            let tag = if line.suspended == Some(writer) { tags.continuation } else { tags.prefix };
            if !kernel {
                line.suspended = None;
            }
            if !tag.is_empty() {
                emit(tag);
            }
        }
        emit(fragment);
        line.owner = if fragment.ends_with('\n') { LINE_START } else { writer };
        rest = tail;
    }
}

/// Write one Cell console record owned by `writer`, tagged `USER: ` per line
/// (`USER+ ` on the line resuming a line a kernel record interrupted).
///
/// USER stdout (cell `println`/`sys_log`) MUST always appear regardless of the
/// kernel's `log::max_level` — it is application output, not kernel debug chatter.
/// Routing it through `log::info!` (as `print_user_log` once did) meant lowering
/// the kernel log level to silence boot spam also silenced the shell prompt.
///
/// `writer` is the calling task's id. A record that would land inside another
/// writer's open line first emits the newline that closes it — one line, one
/// writer, one tag.
pub fn write_user_record(writer: usize, msg: &str) {
    let mut line = CONSOLE.lock();
    emit_record(&mut line, writer, &USER_TAGS, msg, |s| {
        let _ = DirectWriter.write_str(s);
    });
}

/// One kernel `log` record: holds the console for the whole record so its bytes
/// are emitted contiguously, and closes a foreign open line before its first byte.
struct ConsoleRecord<'a> {
    state: crate::sync::SpinlockGuard<'a, ConsoleLine>,
}

impl fmt::Write for ConsoleRecord<'_> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        emit_record(&mut self.state, KERNEL_LOG_OWNER, &KERNEL_TAGS, s, |s| {
            let _ = DirectWriter.write_str(s);
        });
        Ok(())
    }
}

/// Begin a kernel `log` record (the kernel logger's entry point).
fn begin_record() -> ConsoleRecord<'static> {
    ConsoleRecord { state: CONSOLE.lock() }
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
            let mut out = begin_record();
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
    let mut input = RX_BUFFER.lock();
    #[cfg(cellos_p01_trace)]
    let _semantic = crate::memory::heap::p01_owner::creation(None,
        crate::memory::heap::p01_owner::DRIVER_SHARED);
    *input = Some(VecDeque::with_capacity(MAX_RX_BUFFERED));
    #[cfg(cellos_p01_trace)]
    drop(_semantic);
    drop(input);
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
/// Pushes received bytes, unchanged, into RX_BUFFER. CR is not rewritten: the only
/// reader (`console_drv`) relays raw bytes to the input service and converts CR→LF
/// itself in the legacy no-input-service fallback.
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
    use super::{emit_record, ConsoleLine, KERNEL_LOG_OWNER, KERNEL_TAGS, USER_TAGS};
    use alloc::string::String;
    use alloc::vec::Vec;

    /// The bytes a real console would receive for one Cell record, through the
    /// same loop an image runs — only the UART is replaced by a `String`.
    fn record(line: &mut ConsoleLine, writer: usize, msg: &str) -> String {
        let mut out = String::new();
        emit_record(line, writer, &USER_TAGS, msg, |s| out.push_str(s));
        out
    }

    /// One kernel `log` record as the logger emits it: `writeln!` hands the
    /// formatted line and its newline to `ConsoleRecord` as separate fragments.
    fn kernel(line: &mut ConsoleLine, text: &str) -> String {
        let mut out = String::new();
        for fragment in [text, "\n"] {
            emit_record(line, KERNEL_LOG_OWNER, &KERNEL_TAGS, fragment, |s| out.push_str(s));
        }
        out
    }

    /// Rebuild one Cell's bytes from a console capture by the documented rule:
    /// drop kernel lines, strip the line-start `USER: ` tag, and join every
    /// `USER+ ` line onto the Cell line before it. Panics on any physical line
    /// that does not start with exactly one known tag.
    fn rebuild(console: &str) -> String {
        let mut out = String::new();
        for physical in console.split_inclusive('\n') {
            if physical.starts_with('[') {
                continue;
            }
            if let Some(body) = physical.strip_prefix("USER: ") {
                out.push_str(body);
            } else if let Some(body) = physical.strip_prefix("USER+ ") {
                // The forced break that preceded the kernel line is not Cell output.
                assert_eq!(out.pop(), Some('\n'), "continuation without an interrupted line");
                out.push_str(body);
            } else {
                panic!("untagged physical line {physical:?}");
            }
        }
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

    /// The phase03 defect: a record whose unterminated tail follows an inner
    /// newline left the console looking "at a line start", so the writer's next
    /// record (the next 512-byte relay chunk) re-inserted `USER: ` mid-line and a
    /// kernel record was spliced into the open line without a break.
    #[test]
    fn tail_after_inner_newline_keeps_the_line_open() {
        let mut line = ConsoleLine::new();
        assert_eq!(record(&mut line, 7, "{\"a\":1}\n{\"b\""), "USER: {\"a\":1}\nUSER: {\"b\"");
        assert_eq!(record(&mut line, 7, ":2}\n"), ":2}\n");
        assert_eq!(record(&mut line, 7, "{\"c\""), "USER: {\"c\"");
        assert_eq!(kernel(&mut line, "[ INFO] Syscall::Exit"), "\n[ INFO] Syscall::Exit\n");
    }

    /// A record that writes nothing neither claims nor closes the line.
    #[test]
    fn empty_record_changes_nothing() {
        let mut line = ConsoleLine::new();
        assert_eq!(record(&mut line, 7, ""), "");
        assert_eq!(kernel(&mut line, "[ INFO] k"), "[ INFO] k\n");
        assert_eq!(record(&mut line, 7, "ab"), "USER: ab");
        assert_eq!(record(&mut line, 8, ""), "");
        assert_eq!(record(&mut line, 7, "c\n"), "c\n");
    }

    /// The board defect the owner discipline exists for: the guest echoes its
    /// command one byte per `sys_log` from the hypervisor, so a diagnostic from
    /// another cell used to be spliced into the middle of the line the operator
    /// was typing (`USER: ping -c 3 192.16[net-loop] turns=215 …`). A foreign Cell
    /// closes that line first; the guest resumes on a plain `USER: ` line, since
    /// another Cell's line now sits between the two halves.
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
        assert_eq!(record(&mut line, GUEST, "8.42.1"), "USER: 8.42.1");
    }

    /// A kernel line interrupting an open Cell line starts on its own line, and
    /// the Cell's continuation after it is tagged `USER+ ` — once — after which
    /// the Cell's later lines are plain `USER: ` again.
    #[test]
    fn kernel_line_breaks_and_marks_the_continuation() {
        let mut line = ConsoleLine::new();
        assert_eq!(record(&mut line, 5, "Cellos > "), "USER: Cellos > ");
        assert_eq!(kernel(&mut line, "[ INFO] x"), "\n[ INFO] x\n");
        assert_eq!(kernel(&mut line, "[ WARN] y"), "[ WARN] y\n");
        assert_eq!(record(&mut line, 5, "vfs\nnext"), "USER+ vfs\nUSER: next");
        assert_eq!(kernel(&mut line, "[ INFO] z"), "\n[ INFO] z\n");
        assert_eq!(record(&mut line, 5, "\n"), "USER+ \n");
        assert_eq!(record(&mut line, 5, "done\n"), "USER: done\n");
    }

    /// A suspended Cell whose line is followed by another Cell's line resumes
    /// with a plain `USER: `: the continuation tag only ever joins across
    /// kernel lines.
    #[test]
    fn another_cell_ends_the_suspension() {
        let mut line = ConsoleLine::new();
        assert_eq!(record(&mut line, 5, "half"), "USER: half");
        assert_eq!(kernel(&mut line, "[ INFO] k"), "\n[ INFO] k\n");
        assert_eq!(record(&mut line, 6, "other\n"), "USER: other\n");
        assert_eq!(record(&mut line, 5, "rest\n"), "USER: rest\n");
    }

    /// End to end over the shape of the phase03 capture: a long multi-line
    /// stream relayed in 512-byte records, with kernel lines landing between
    /// records (mid-line and at line starts). Every physical line carries
    /// exactly one tag at its start, and the documented rebuild rule recovers
    /// the Cell's bytes exactly.
    #[test]
    fn chunked_stream_with_kernel_lines_rebuilds_exactly() {
        let mut stream = String::new();
        for i in 0..40 {
            stream.push_str(&alloc::format!(
                "{{\"frame\":\"row\",\"value\":{{\"cell_id\":{i},\"name\":\"cell-{i}\",\"pad\":\"{}\"}}}}\n",
                "x".repeat(i * 7 % 61)
            ));
        }
        stream.push_str("unterminated tail");
        let bytes = stream.as_bytes();
        let chunks: Vec<&str> = bytes
            .chunks(512)
            .map(|c| core::str::from_utf8(c).unwrap())
            .collect();
        assert!(chunks.len() > 4);
        let mut line = ConsoleLine::new();
        let mut console = String::new();
        for (n, chunk) in chunks.iter().enumerate() {
            console.push_str(&record(&mut line, 7, chunk));
            if n % 2 == 1 {
                console.push_str(&kernel(&mut line, &alloc::format!("[ INFO] Syscall::Exit: task {n}")));
            }
        }
        console.push_str(&kernel(&mut line, "[ INFO] end"));
        for physical in console.lines() {
            let body = physical
                .strip_prefix("USER: ")
                .or_else(|| physical.strip_prefix("USER+ "))
                .or_else(|| physical.strip_prefix("[ INFO] "))
                .unwrap_or_else(|| panic!("untagged physical line {physical:?}"));
            assert!(!body.contains("USER"), "tag inside a line: {physical:?}");
            assert!(!body.contains("[ INFO]"), "kernel record spliced: {physical:?}");
        }
        assert!(console.contains("\nUSER+ "), "fixture must exercise a mid-line kernel interrupt");
        assert_eq!(rebuild(&console), stream + "\n");
    }
}
