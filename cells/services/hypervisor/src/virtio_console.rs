#![allow(dead_code)]
//! virtio-console device model (DeviceID=3), one port, no optional features.
//!
//! aarch64 (virtio-mmio slot 0 → SPI 16): the transmitq is the guest's log,
//! printed to ViCell serial; the receiveq is a stub.
//! x86 (slot 2, the Chrome guest's `/dev/hvc0`): both queues carry clipboard
//! lines ([`ClipboardPort`]); completions raise ISR status only and the run
//! loop delivers them on its display IRQ.

extern crate alloc;
use alloc::vec;
use ostd::io::print;

use crate::clipboard_port::ClipboardPort;
use crate::virtio_mmio::{QueueCfg, VirtioDevice};
use crate::virtqueue::{process_notify, DescBuf};

/// SPI line for virtio-mmio slot 0.
#[cfg(target_arch = "aarch64")]
const CONSOLE_SPI: u32 = 16;
/// Most bytes taken from one clipboard transmit buffer: a whole line fits
/// (`clipboard_line::MAX_LINE`), and a guest posting huge buffers cannot make
/// one notify walk gigabytes.
const CLIPBOARD_TX_MAX: usize = 128 * 1024;
const TX_CHUNK: usize = 4096;

pub struct Console {
    last_avail_rx: u16,
    last_avail_tx: u16,
    used_rx: u16,
    used_tx: u16,
    /// Set on x86: the queues carry clipboard lines instead of a log.
    clipboard: Option<ClipboardPort>,
}

impl Console {
    /// The log console (aarch64).
    pub const fn new() -> Self {
        Self {
            last_avail_rx: 0,
            last_avail_tx: 0,
            used_rx: 0,
            used_tx: 0,
            clipboard: None,
        }
    }

    /// The clipboard console (x86 Chrome guest).
    pub const fn clipboard() -> Self {
        Self {
            last_avail_rx: 0,
            last_avail_tx: 0,
            used_rx: 0,
            used_tx: 0,
            clipboard: Some(ClipboardPort::new()),
        }
    }

    /// The clipboard side, on a clipboard console.
    pub fn clipboard_port(&mut self) -> Option<&mut ClipboardPort> {
        self.clipboard.as_mut()
    }

    /// Deliver queued clipboard bytes into the receiveq (queue 0). Returns
    /// whether used entries were published; the caller signals them.
    pub fn flush_rx(&mut self, qcfg: &QueueCfg, vm_id: usize) -> bool {
        let Some(port) = self.clipboard.as_mut() else {
            return false;
        };
        port.fill_rx(qcfg, vm_id, &mut self.last_avail_rx, &mut self.used_rx)
    }
}

/// Feed one transmit buffer of the clipboard console to `port`; bytes read.
fn clipboard_tx(port: &mut ClipboardPort, vm_id: usize, buf: &DescBuf) -> u32 {
    let len = (buf.len as usize).min(CLIPBOARD_TX_MAX);
    let mut chunk = [0u8; TX_CHUNK];
    let mut done = 0;
    while done < len {
        let n = (len - done).min(TX_CHUNK);
        let gpa = buf.gpa + done as u64;
        if crate::vmm::read_guest_memory(vm_id, gpa, &mut chunk[..n]) != n {
            break;
        }
        port.tx(&chunk[..n]);
        done += n;
    }
    done as u32
}

impl VirtioDevice for Console {
    fn device_id(&self) -> u32 {
        3
    }
    fn notify(&mut self, q: usize, qcfg: &QueueCfg, vm_id: usize, vcpu_id: usize) -> bool {
        match q {
            // RX queue (host→guest): the guest added buffers.
            0 => self.flush_rx(qcfg, vm_id),
            1 => {
                // TX queue (guest→host): read each readable descriptor.
                let clipboard = &mut self.clipboard;
                let published = process_notify(
                    vm_id,
                    qcfg,
                    &mut self.last_avail_tx,
                    &mut self.used_tx,
                    |bufs: &[DescBuf]| {
                        let mut total = 0u32;
                        for buf in bufs {
                            if buf.writable {
                                continue;
                            } // TX bufs are driver-readable
                            if let Some(port) = clipboard.as_mut() {
                                total += clipboard_tx(port, vm_id, buf);
                                continue;
                            }
                            let cap = buf.len.min(4096) as usize;
                            let mut data = vec![0u8; cap];
                            let n = crate::vmm::read_guest_memory(vm_id, buf.gpa, &mut data);
                            if n == 0 || n == usize::MAX {
                                continue;
                            }
                            if let Ok(s) = core::str::from_utf8(&data[..n]) {
                                print(s);
                            }
                            total += n as u32;
                        }
                        total
                    },
                );
                if published > 0 {
                    // Inject SPI so the guest interrupt handler runs and processes the used ring.
                    #[cfg(target_arch = "aarch64")]
                    crate::vmm::inject_irq(vm_id, vcpu_id, crate::vmm::device_irq(CONSOLE_SPI));
                    // x86: `VirtioMmio` sets ISR status; the run loop raises the line.
                    #[cfg(not(target_arch = "aarch64"))]
                    let _ = vcpu_id;
                    true
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    fn reset(&mut self) {
        self.last_avail_rx = 0;
        self.last_avail_tx = 0;
        self.used_rx = 0;
        self.used_tx = 0;
        if let Some(port) = self.clipboard.as_mut() {
            port.reset();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_console_tx_notify_publishes_no_entries_and_returns_false() {
        let mut console = Console::new();
        let qcfg = QueueCfg::default();
        let published = console.notify(1, &qcfg, 0, 0);
        assert!(!published);
    }

    #[test]
    fn clipboard_console_rx_without_ready_queue_publishes_nothing() {
        let mut console = Console::clipboard();
        console.clipboard_port().unwrap().queue_for_guest("text");
        assert!(!console.notify(0, &QueueCfg::default(), 0, 0));
    }
}
