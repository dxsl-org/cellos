//! Fault injection for the two-NIC witness (`tests/integration/tests/nic2-x86.rs`).
//!
//! Compiled only with the `fault-inject` feature, which no production image
//! enables: `scripts/x86/make-nic2-iso.sh` builds its own NIC cells with it
//! under a separate cargo target directory. A transmitted frame carrying
//! `F18-DIE-pci-BB-DD-F`, naming this cell's own function the way the kernel
//! logs NIC routes, makes the cell exit 1 on the spot: no reply, no device
//! drop, so the controller is still programmed and the kernel's retirement
//! path has to quiesce it. A suite shell reaches the marker without any extra
//! tool, because `nc F18-DIE-pci-00-03-0 53` puts the hostname into the DNS
//! query the net service transmits.
//!
//! The name binds the death to one device: a DNS retransmit that leaves
//! through another NIC after the net service moves to it does not kill that
//! NIC too.

use core::sync::atomic::{AtomicU32, Ordering};
use ostd::io::print_fmt;
use ostd::syscall::sys_exit;

/// No function bound yet.
const UNBOUND: u32 = u32::MAX;

static BOUND_BDF: AtomicU32 = AtomicU32::new(UNBOUND);

const PREFIX: &[u8; 12] = b"F18-DIE-pci-";
/// `PREFIX` plus `BB-DD-F`.
const MARKER_LEN: usize = PREFIX.len() + 7;

/// Remember the function this cell owns; frames are checked from here on.
pub(crate) fn bound(bdf: u32) {
    BOUND_BDF.store(bdf, Ordering::Relaxed);
}

/// Exit 1 before transmitting `frame` when it carries this cell's marker.
pub(crate) fn before_tx(tag: &str, frame: &[u8]) {
    let bdf = BOUND_BDF.load(Ordering::Relaxed);
    if bdf == UNBOUND {
        return;
    }
    let marker = marker(bdf);
    if !carries(frame, &marker) {
        return;
    }
    let name = core::str::from_utf8(&marker).unwrap_or("?");
    let _ = print_fmt(format_args!(
        "{} fault-inject: {} transmitted, driver exiting\n",
        tag, name
    ));
    sys_exit(1)
}

/// `F18-DIE-pci-BB-DD-F` for the requester ID `bdf` (bus, device, function as
/// `kernel/src/dev/driver_cell.rs` `DeviceName` prints them).
fn marker(bdf: u32) -> [u8; MARKER_LEN] {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let hex = |value: u32| HEX[(value & 0xf) as usize];
    let (bus, device, function) = ((bdf >> 8) & 0xff, (bdf >> 3) & 0x1f, bdf & 7);
    let mut out = [0u8; MARKER_LEN];
    out[..PREFIX.len()].copy_from_slice(PREFIX);
    out[PREFIX.len()..].copy_from_slice(&[
        hex(bus >> 4),
        hex(bus),
        b'-',
        hex(device >> 4),
        hex(device),
        b'-',
        hex(function),
    ]);
    out
}

fn carries(frame: &[u8], marker: &[u8]) -> bool {
    frame.windows(marker.len()).any(|window| window == marker)
}

#[cfg(test)]
mod tests {
    use super::{carries, marker};

    #[test]
    fn marker_names_the_function_as_the_kernel_logs_it() {
        assert_eq!(&marker(0x18), b"F18-DIE-pci-00-03-0");
        assert_eq!(&marker(0x0123), b"F18-DIE-pci-01-04-3");
        assert_eq!(&marker(0xff_ff), b"F18-DIE-pci-ff-1f-7");
    }

    #[test]
    fn only_this_functions_marker_fires() {
        // A DNS query: the hostname is one length-prefixed label.
        let mut frame = vec![0u8; 54];
        frame.push(19);
        frame.extend_from_slice(b"F18-DIE-pci-00-03-0");
        frame.extend_from_slice(&[0, 0, 1, 0, 1]);
        assert!(carries(&frame, &marker(0x18)));
        assert!(!carries(&frame, &marker(0x20)), "the other NIC survives");
        assert!(!carries(&frame[..60], &marker(0x18)), "truncated marker");
    }
}
