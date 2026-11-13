use crate::sync::Spinlock;
use abi::input_producer::{EV_ASCII, PRODUCER_FRAME_LEN};
use alloc::collections::VecDeque;
use core::sync::atomic::Ordering;

#[cfg(target_arch = "aarch64")]
use crate::hal::backend::ArmPlatform as _;

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
        let input_tid = crate::dev::driver_cell::INPUT_CELL_TID.load(Ordering::Relaxed);

        // When input service is online, bytes are relayed via IPC — never buffered.
        // Only apply the buffer-full early-out when operating in fallback buffered mode.
        if input_tid == 0 && self.buffer.len() >= Self::MAX_BUFFERED {
            return false;
        }
        let mut received = false;

        // Retain exactly the failed event and stop reading hardware until IPC
        // accepts it. This keeps releases and packet ordering under backpressure
        // without an allocating or unbounded kernel backlog.
        if input_tid != 0 {
            let mut pending = PENDING_ASCII.lock();
            if let Some(b) = *pending {
                if !relay_ascii_to_input(input_tid, b) {
                    return false;
                }
                *pending = None;
                received = true;
            }
            #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
            {
                let mut pending = PENDING_PS2.lock();
                if let Some(event) = *pending {
                    if !relay_evdev_to_input(input_tid, event.opcode, event.code, event.value) {
                        return received;
                    }
                    *pending = None;
                    received = true;
                }
            }
        }

        // Serial is text-only EV_ASCII; physical PS/2 keys use EV_KEY below.
        // With no input service, retain the buffered sys_read(fd=0) fallback.
        // `$ring` marks bytes drained from the UART RX ring, which stores them raw.
        macro_rules! route_byte {
            ($c:expr, $ring:expr) => {
                match route_for(input_tid != 0, $c, $ring) {
                    Route::Relay(c) => {
                        if !relay_ascii_to_input(input_tid, c) {
                            *PENDING_ASCII.lock() = Some(c);
                            return true; // stop draining; order preserved via backlog
                        }
                    }
                    Route::Buffer(c) => {
                        if self.buffer.len() < Self::MAX_BUFFERED {
                            #[cfg(feature = "p01-trace")]
                            let _semantic = crate::memory::heap::p01_owner::creation(
                                None,
                                crate::memory::heap::p01_owner::DRIVER_SHARED,
                            );
                            self.buffer.push_back(c);
                        }
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
            let Some(c) = crate::dev::uart::poll_rhr() else {
                break;
            };
            route_byte!(c, false);
        }

        // 1b. Drain any chars the UART IRQ handler buffered (when IRQs reach S-mode).
        // This path is also only relevant for RISC-V; on AArch64 IRQ-buffered
        // chars come through the PL011 path below.
        #[cfg(any(target_arch = "riscv64", target_arch = "riscv32"))]
        while self.buffer.len() < Self::MAX_BUFFERED || input_tid != 0 {
            let Some(c) = crate::dev::uart::getchar() else {
                break;
            };
            route_byte!(c, true);
        }

        // 1c. Board-selected AArch64 console RX. Interrupt-driven boards first
        // drain the ring the UART IRQ handler filled.
        #[cfg(target_arch = "aarch64")]
        if crate::hal::backend::Board::CONSOLE_RX_IRQ {
            while self.buffer.len() < Self::MAX_BUFFERED || input_tid != 0 {
                let Some(c) = crate::dev::uart::getchar() else {
                    break;
                };
                route_byte!(c, true);
            }
        }

        // Then poll the console UART directly. On interrupt-driven boards this is
        // the fallback for early boot and lost IRQ routing. On QEMU virt the PL011
        // at 0x0900_0000 is the only path; `-serial tcp:...` connects its TX/RX to
        // the TCP socket used by the integration-test harness.
        #[cfg(target_arch = "aarch64")]
        while self.buffer.len() < Self::MAX_BUFFERED || input_tid != 0 {
            let Some(c) = crate::hal::backend::Board::console_poll_rx() else {
                break;
            };
            route_byte!(c, false);
        }

        // 1d. Drain IRQ-filled RX buffer on x86_64.
        // vi_handle_uart_irq() (fired by IOAPIC IRQ 4 / IDT vector 0x24) pushes
        // COM1 bytes into uart::RX_BUFFER; we drain it here on every poll call
        // so the blocking file_read(fd=0) loop eventually finds a byte.
        #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
        while self.buffer.len() < Self::MAX_BUFFERED || input_tid != 0 {
            let Some(c) = crate::dev::uart::getchar() else {
                break;
            };
            route_byte!(c, true);
        }
        // 1e. A single PS/2 reader demultiplexes keyboard and AUX bytes. Never
        // translate physical keys into ASCII while service-input owns input.
        #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
        if input_tid != 0 {
            // Bound work per tick even when the device continuously reports.
            for _ in 0..64 {
                let Some(event) = crate::dev::ps2_keyboard::poll_event() else {
                    break;
                };
                if !relay_evdev_to_input(input_tid, event.opcode, event.code, event.value) {
                    *PENDING_PS2.lock() = Some(event);
                    return true;
                }
                received = true;
            }
        } else {
            while self.buffer.len() < Self::MAX_BUFFERED {
                let Some(c) = crate::dev::ps2_keyboard::poll() else {
                    break;
                };
                route_byte!(c, false);
            }
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

/// Where one received serial byte goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Route {
    /// Relayed to the input service exactly as received.
    Relay(u8),
    /// Queued for the legacy `sys_read(fd=0)` fallback.
    Buffer(u8),
}

/// Pure routing rule. The UART RX ring stores bytes unchanged (Enter is CR, Ctrl-J is
/// LF), so the input service sees raw bytes; only the legacy fallback, which runs when
/// no input service is registered, turns ring-sourced CR into LF as `sys_read` readers
/// have always seen it. Bytes that did not pass through the ring are never rewritten.
pub(crate) fn route_for(input_online: bool, byte: u8, from_rx_ring: bool) -> Route {
    if input_online {
        Route::Relay(byte)
    } else if from_rx_ring && byte == b'\r' {
        Route::Buffer(b'\n')
    } else {
        Route::Buffer(byte)
    }
}

/// One failed serial delivery. No further hardware bytes are consumed until it
/// posts successfully; a fixed slot suffices and never allocates.
static PENDING_ASCII: Spinlock<Option<u8>> =
    Spinlock::ranked(None, crate::sync::rank::PENDING_ASCII);

/// One failed typed delivery, including releases and individual motion axes.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
static PENDING_PS2: Spinlock<Option<crate::dev::ps2_keyboard::Ps2Event>> =
    Spinlock::ranked(None, crate::sync::rank::PENDING_PS2);

/// Relay a UART byte to the input service as one EV_ASCII press event.
///
/// Uses the kernel's bounded nonblocking mailbox so short bursts can be queued
/// while the input service is delivering to the focused cell. A blocked byte
/// remains in `PENDING_ASCII`; later bytes stay in the UART RX ring/FIFO.
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
    // Use isize::MAX as sender_id — distinguishes kernel UART messages from
    // real timeout (Ok(0)) in the input service. Must be isize::MAX (not
    // usize::MAX) because syscall() returns isize: usize::MAX == -1 as isize
    // which causes sys_recv_timeout to return Err instead of Ok.
    const KERNEL_UART_SENDER: usize = isize::MAX as usize;
    let mut msg = [0u8; PRODUCER_FRAME_LEN];
    msg[0] = EV_ASCII;
    msg[1..5].copy_from_slice(&(byte as u32).to_le_bytes());
    msg[5..9].copy_from_slice(&1u32.to_le_bytes()); // press
    crate::task::ipc_post_nonblock(KERNEL_UART_SENDER, input_tid, &msg).is_ok()
}
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
fn relay_evdev_to_input(input_tid: usize, opcode: u8, code: u32, value: i32) -> bool {
    const KERNEL_SENDER: usize = isize::MAX as usize;
    let mut msg = [0u8; PRODUCER_FRAME_LEN];
    msg[0] = opcode;
    msg[1..5].copy_from_slice(&code.to_le_bytes());
    msg[5..9].copy_from_slice(&(value as u32).to_le_bytes());
    crate::task::ipc_post_nonblock(KERNEL_SENDER, input_tid, &msg).is_ok()
}

pub static CONSOLE: Spinlock<viConsole> = Spinlock::ranked(
    viConsole {
        buffer: VecDeque::new(),
    },
    crate::sync::rank::CONSOLE_DRV,
);

pub fn init() {
    // Nothing special to init for SBI Console so far
    // But we might want to clear buffer
    let mut cons = CONSOLE.lock();
    cons.buffer.clear();
    log::info!("Console: Input Driver Initialized.");
}
