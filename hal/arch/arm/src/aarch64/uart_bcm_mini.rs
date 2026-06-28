//! BCM2837 Mini UART (UART1 / AUX peripheral) — Raspberry Pi 3.
//!
//! GPIO14 = TX (alt-func 5), GPIO15 = RX (alt-func 5).
//! Baud rate: 115200 at BCM2837 system clock 250 MHz → AUX_MU_BAUD = 270.
//!
//! Call `init()` from `kmain` before any logging. The mini UART is always
//! available on GPIO14/15 once `enable_uart=1` is set in `config.txt`
//! (VideoCore pre-configures the pin-mux; `init()` also does it explicitly).

const AUX_BASE:    usize = 0x3F21_5000;
const AUX_ENABLES: usize = AUX_BASE + 0x004;
const AUX_MU_IO:   usize = AUX_BASE + 0x040;
const AUX_MU_IER:  usize = AUX_BASE + 0x044;
const AUX_MU_IIR:  usize = AUX_BASE + 0x048;
const AUX_MU_LCR:  usize = AUX_BASE + 0x04C;
const AUX_MU_MCR:  usize = AUX_BASE + 0x050;
const AUX_MU_LSR:  usize = AUX_BASE + 0x054;
const AUX_MU_CNTL: usize = AUX_BASE + 0x060;
const AUX_MU_BAUD: usize = AUX_BASE + 0x068;

// BCM GPIO base for pin-mux setup.
const GPIO_BASE:  usize = 0x3F20_0000;
const GPFSEL1:    usize = GPIO_BASE + 0x004; // function select for GPIO 10-19

// LSR bit 5: TX FIFO has space.  Bit 0: RX data ready.
const LSR_TX_EMPTY: u32 = 1 << 5;
const LSR_RX_READY: u32 = 1 << 0;

#[inline(always)]
fn wr(addr: usize, val: u32) {
    // SAFETY: bare-metal MMIO; all callers hold the single-core invariant at boot.
    unsafe { core::ptr::write_volatile(addr as *mut u32, val) }
}
#[inline(always)]
fn rd(addr: usize) -> u32 {
    // SAFETY: same as wr.
    unsafe { core::ptr::read_volatile(addr as *const u32) }
}

/// Configure GPIO14 (TX) and GPIO15 (RX) for Alt-Function 5 (mini UART).
///
/// Required when VideoCore has not already set the pin-mux (i.e., when
/// `enable_uart=1` is absent from `config.txt` on the SD card).
fn init_gpio_mux() {
    let mut v = rd(GPFSEL1);
    // GPIO14: bits [14:12] = 010 = Alt5
    v &= !(7 << 12);
    v |=   2 << 12;
    // GPIO15: bits [17:15] = 010 = Alt5
    v &= !(7 << 15);
    v |=   2 << 15;
    wr(GPFSEL1, v);
}

/// Initialize BCM2837 mini UART at 115200 8N1.
///
/// # Safety
/// Bare-metal init; must be called once from `kmain` before any concurrent
/// access. No cell is running at this point.
pub fn init() {
    init_gpio_mux();

    // 1. Enable mini UART in AUX block (bit 0).
    wr(AUX_ENABLES, 1);

    // 2. Disable TX/RX while configuring.
    wr(AUX_MU_CNTL, 0);

    // 3. Disable interrupts (polled I/O only).
    wr(AUX_MU_IER, 0);

    // 4. 8-bit mode.
    wr(AUX_MU_LCR, 0b11);

    // 5. RTS not driven by modem.
    wr(AUX_MU_MCR, 0);

    // 6. Clear TX/RX FIFOs.
    wr(AUX_MU_IIR, 0xC6);

    // 7. Baud rate: (250_000_000 / (8 * 115_200)) − 1 = 270.
    wr(AUX_MU_BAUD, 270);

    // 8. Enable TX + RX.
    wr(AUX_MU_CNTL, 0b11);
}

/// Blocking write — waits until TX FIFO has space, then sends one byte.
pub fn putchar(byte: u8) {
    while rd(AUX_MU_LSR) & LSR_TX_EMPTY == 0 {}
    wr(AUX_MU_IO, byte as u32);
}

/// Blocking write of a string — calls `putchar` for each byte.
pub fn puts(s: &str) {
    for b in s.bytes() { putchar(b); }
}

/// FIFO-safe probe write — waits for TX FIFO space then sends one byte.
///
/// Use instead of raw `write_volatile(AUX_MU_IO, ...)` in debug probes so bytes
/// are never dropped when a prior log message is still draining the TX FIFO.
#[inline]
pub fn probe_put(byte: u8) {
    while rd(AUX_MU_LSR) & LSR_TX_EMPTY == 0 {}
    wr(AUX_MU_IO, byte as u32);
}

/// Non-blocking read — returns `Some(byte)` when RX FIFO has data.
pub fn poll_rx() -> Option<u8> {
    if rd(AUX_MU_LSR) & LSR_RX_READY != 0 {
        Some((rd(AUX_MU_IO) & 0xFF) as u8)
    } else {
        None
    }
}
