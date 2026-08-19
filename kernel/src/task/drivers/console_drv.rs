use crate::sync::Spinlock;
use alloc::collections::VecDeque;
use core::sync::atomic::Ordering;

#[allow(non_camel_case_types)]
pub struct viConsole {
    pub buffer: VecDeque<u8>,
}

impl Default for viConsole {
    fn default() -> Self {
        Self::new()
    }
}

impl viConsole {
    pub fn new() -> Self {
        Self {
            buffer: VecDeque::new(),
        }
    }

    /// Hard cap on buffered input bytes. A line-oriented console never needs
    /// more than this; the cap prevents a misbehaving input source (e.g. an
    /// SBI/IRQ path that returns phantom bytes every poll) from growing the
    /// VecDeque unboundedly and exhausting the kernel heap while a reader spins.
    const MAX_BUFFERED: usize = 4096;

    /// Polls input sources and pushes available characters to the buffer.
    /// Returns true if a character was received.
    pub fn poll(&mut self) -> bool {
        let input_tid = crate::task::drivers::driver_cell::INPUT_CELL_TID.load(Ordering::Relaxed);

        // When input service is online, bytes are relayed via IPC — never buffered.
        // Only apply the buffer-full early-out when operating in fallback buffered mode.
        if input_tid == 0 && self.buffer.len() >= Self::MAX_BUFFERED {
            return false;
        }
        let mut received = false;

        // Backpressure: relay any bytes that failed to post on a previous tick
        // BEFORE reading new ones, so byte order is preserved. If the input
        // service queue is still full, leave the backlog (and the HW FIFO)
        // alone — QEMU's chardev applies TCP backpressure while the FIFO is
        // full, so nothing is lost upstream either.
        if input_tid != 0 {
            let mut pending = PENDING_ASCII.lock();
            while let Some(&b) = pending.front() {
                if relay_ascii_to_input(input_tid, b) {
                    pending.pop_front();
                    received = true;
                } else {
                    return received; // input queue still full — retry next tick
                }
            }
        }

        // Route rule: when the input service is online (input_tid != 0), relay
        // bytes to it exclusively via EV_ASCII IPC — do NOT push to self.buffer.
        // This prevents double delivery: the shell reads via input service events,
        // and without this guard any app calling sys_read(fd=0) would also see the
        // same bytes from self.buffer, causing duplicate input.
        // When input service is offline (input_tid == 0), bytes go to self.buffer
        // only, keeping the sys_read(fd=0) fallback path working for early boot.
        //
        // A failed relay (input service pending_msgs full during a paste-speed
        // burst) parks the byte in PENDING_ASCII and stops draining — dropping
        // it instead silently lost mid-line characters ("vappend /data/…" arrived
        // as "vappend ta/…") whenever a burst outpaced the input service.
        macro_rules! route_byte {
            ($c:expr) => {
                let c = $c;
                if input_tid != 0 {
                    if !relay_ascii_to_input(input_tid, c) {
                        PENDING_ASCII.lock().push_back(c);
                        return true; // stop draining; order preserved via backlog
                    }
                } else {
                    if self.buffer.len() < Self::MAX_BUFFERED {
                        self.buffer.push_back(c);
                    }
                }
                received = true;
            };
        }

        // 1a. Directly poll the 16550 RHR — RISC-V QEMU virt only.
        // The 16550 lives at 0x10_000_000 on RISC-V; that address is not a
        // 16550 on AArch64 (UART is PL011 at 0x0900_0000). Reading it there
        // returns garbage (0xFF), causing continuous `?` spam.
        #[cfg(any(target_arch = "riscv64", target_arch = "riscv32"))]
        while self.buffer.len() < Self::MAX_BUFFERED || input_tid != 0 {
            let Some(c) = crate::task::drivers::uart::poll_rhr() else {
                break;
            };
            route_byte!(c);
        }

        // 1b. Drain any chars the UART IRQ handler buffered (when IRQs reach S-mode).
        // This path is also only relevant for RISC-V; on AArch64 IRQ-buffered
        // chars come through the PL011 path below.
        #[cfg(any(target_arch = "riscv64", target_arch = "riscv32"))]
        while self.buffer.len() < Self::MAX_BUFFERED || input_tid != 0 {
            let Some(c) = crate::task::drivers::uart::getchar() else {
                break;
            };
            route_byte!(c);
        }

        // 1c. Poll the board-selected AArch64 UART RX backend.
        #[cfg(all(target_arch = "aarch64", feature = "board-rpi3"))]
        while self.buffer.len() < Self::MAX_BUFFERED || input_tid != 0 {
            let Some(c) = crate::task::drivers::uart::getchar() else {
                break;
            };
            route_byte!(c);
        }

        // Keep direct polling as a fallback for early boot and lost IRQ routing.
        #[cfg(all(target_arch = "aarch64", feature = "board-rpi3"))]
        while self.buffer.len() < Self::MAX_BUFFERED || input_tid != 0 {
            let Some(c) = crate::hal::uart_bcm_mini::poll_rx() else {
                break;
            };
            route_byte!(c);
        }

        // QEMU virt maps PL011 at 0x0900_0000; `-serial tcp:...` connects its
        // TX/RX to the TCP socket used by the integration-test harness.
        #[cfg(all(target_arch = "aarch64", not(feature = "board-rpi3")))]
        while self.buffer.len() < Self::MAX_BUFFERED || input_tid != 0 {
            let Some(c) = crate::hal::uart_pl011::poll_rx() else {
                break;
            };
            route_byte!(c);
        }

        // 1d. Drain IRQ-filled RX buffer on x86_64.
        // vi_handle_uart_irq() (fired by IOAPIC IRQ 4 / IDT vector 0x24) pushes
        // COM1 bytes into uart::RX_BUFFER; we drain it here on every poll call
        // so the blocking file_read(fd=0) loop eventually finds a byte.
        #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
        while self.buffer.len() < Self::MAX_BUFFERED || input_tid != 0 {
            let Some(c) = crate::task::drivers::uart::getchar() else {
                break;
            };
            route_byte!(c);
        }

        // NOTE: the SBI DBCN console-read fallback was removed — on this QEMU /
        // OpenSBI build it returns phantom bytes on every call, which (combined
        // with a spinning reader) grew the buffer without bound. The direct RHR
        // poll (1a) is the reliable UART input path.

        // VirtIO keyboard/mouse delivery is owned solely by
        // `virtio_input::dispatch_pending`, called from the timer tick BEFORE this
        // poll(). It drains the same event_queue with a proper SUM guard and
        // non-destructive (peek-then-pop-on-delivery) semantics. Draining it here
        // too would (a) double-consume events and (b) ipc_send into the input
        // service's U-mode buffer WITHOUT setting SUM → S-mode store page-fault
        // (scause=15) the moment an event is forwarded from the timer ISR.
        // The UART paths above use relay_ascii_to_input(), which IS SUM-safe.

        received
    }

    /// Read a byte from buffer (Non-blocking)
    pub fn read_byte(&mut self) -> Option<u8> {
        self.buffer.pop_front()
    }
}

/// Bytes whose EV_ASCII post failed because the input service's pending_msgs
/// queue was full (paste-speed burst). Retried at the start of every poll()
/// tick, in order, before any new FIFO bytes are consumed.
static PENDING_ASCII: Spinlock<VecDeque<u8>> = Spinlock::new(VecDeque::new());

/// Relay a UART byte to the input service as one EV_ASCII press event.
///
/// Uses the kernel's 64-message nonblocking post so short bursts are queued while
/// the input service is blocked delivering a keyboard event to the focused cell.
/// Longer bursts remain ordered in `PENDING_ASCII` and the IRQ RX ring until the
/// input service catches up.
///
/// Returns `false` when the PRESS event could not be queued (input service
/// pending_msgs full) — the caller must retain the byte and retry later. The
/// UART ASCII is edge-triggered text input, so only a Press event is emitted.
/// Sending a synthetic Release doubles queue pressure while shells ignore it.
///
/// Delivery copies into an owned kernel mailbox entry. The resumed input task
/// drains that entry into its own user buffer, so this interrupt-side path never
/// enables supervisor access to user pages.
fn relay_ascii_to_input(input_tid: usize, byte: u8) -> bool {
    // Wire opcode 0x04: raw ASCII relay path (distinct from EV_KEY=0/EV_REL=1/EV_ABS=2).
    const WIRE_ASCII: u8 = 0x04;

    // Use isize::MAX as sender_id — distinguishes kernel UART messages from
    // real timeout (Ok(0)) in the input service. Must be isize::MAX (not
    // usize::MAX) because syscall() returns isize: usize::MAX == -1 as isize
    // which causes sys_recv_timeout to return Err instead of Ok.
    const KERNEL_UART_SENDER: usize = isize::MAX as usize;
    let mut msg = [0u8; 9];
    msg[0] = WIRE_ASCII;
    msg[1..5].copy_from_slice(&(byte as u32).to_le_bytes());
    msg[5..9].copy_from_slice(&1u32.to_le_bytes()); // press
    crate::task::ipc_post_nonblock(KERNEL_UART_SENDER, input_tid, &msg[..9]).is_ok()
}

pub static CONSOLE: Spinlock<viConsole> = Spinlock::new(viConsole {
    buffer: VecDeque::new(),
});

pub fn init() {
    // Nothing special to init for SBI Console so far
    // But we might want to clear buffer
    let mut cons = CONSOLE.lock();
    cons.buffer.clear();
    log::info!("Console: Input Driver Initialized.");
}
