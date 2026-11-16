//! Bring-up, the sender gate and NIC wire dispatch.

use crate::{NicDevice, Probe};
use ostd::app::AppEvent;
use ostd::io::print_fmt;
use ostd::service::{sender_is_live_provider, service};
use ostd::sync::Mutex;
use ostd::syscall::{
    sys_exit, sys_find_pcie_device_by_vendor, sys_register_nic_driver, sys_try_send, PcieDeviceInfo,
};
use wire::proto::nic::{NicStatus, OP_GETMAC, OP_RX, OP_TX};

/// Bounded wait for the Platform Cell / kernel ECAM scan to publish the device
/// table. The cell names its controller exactly, so this is not a sibling
/// race: a registration delay fails closed as "not present".
pub const FIND_ATTEMPTS: usize = 200;

/// Largest frame any NIC cell moves (one 2 KiB receive buffer).
pub const FRAME_MAX: usize = 2048;

/// Reply buffer: 2-byte length header plus the largest frame.
const REPLY_CAP: usize = 2 + FRAME_MAX;

/// The cell's device, published once bring-up succeeds.
pub struct State<D>(Mutex<Option<D>>);

impl<D> State<D> {
    pub const fn new() -> Self {
        Self(Mutex::new(None))
    }
}

impl<D> Default for State<D> {
    fn default() -> Self {
        Self::new()
    }
}

/// The NIC cell's event handler (expanded by [`crate::cell!`]).
pub fn on_event<D: NicDevice>(state: &State<D>, event: AppEvent) {
    match event {
        AppEvent::Init => bring_up(state),
        // The net service speaks the raw NIC wire (no 0xAC App-SDK envelope),
        // so requests arrive as RawMessage; Message carries the same payload.
        AppEvent::Message { sender_tid, data } | AppEvent::RawMessage { sender_tid, data } => {
            serve(state, sender_tid, &data)
        }
        AppEvent::Shutdown | AppEvent::ShutdownWith { .. } => sys_exit(0),
        _ => {}
    }
}

fn find<D: NicDevice>() -> Option<PcieDeviceInfo> {
    let mut info = PcieDeviceInfo::zeroed();
    for _ in 0..FIND_ATTEMPTS {
        for &device_id in D::QUERY_IDS {
            // `Ok(false)` is "absent", not found.
            if let Ok(true) = sys_find_pcie_device_by_vendor(D::VENDOR, device_id, &mut info) {
                return Some(info);
            }
        }
        ostd::task::yield_now();
    }
    None
}

fn bring_up<D: NicDevice>(state: &State<D>) {
    let Some(info) = find::<D>() else {
        if let Some(line) = D::ABSENT_LINE {
            let _ = print_fmt(format_args!("{} {}\n", D::TAG, line));
        }
        sys_exit(0);
    };

    // Defense in depth behind the kernel's family gate: the register model
    // is never programmed on an unclassified device.
    let (vendor, device) = (info.vendor_id, info.device_id);
    let Some(identity) = D::classify(vendor, device) else {
        let _ = print_fmt(format_args!(
            "{} unsupported Ethernet {:04x}:{:04x}; driver gate closed\n",
            D::TAG,
            vendor,
            device
        ));
        sys_exit(0);
    };

    let probe = Probe::new(D::TAG, info);
    let dev = match D::init(&probe, identity) {
        Ok(dev) => dev,
        Err(error) => {
            let _ = print_fmt(format_args!(
                "{} controller initialization failed: {:?}\n",
                D::TAG,
                error
            ));
            sys_exit(1)
        }
    };

    // Publish before registration: the kernel may route requests as soon as
    // the syscall succeeds.
    let _ = print_fmt(format_args!(
        "{} controller bound {:04x}:{:04x} {} {}\n",
        D::TAG,
        vendor,
        device,
        D::identity_name(&identity),
        BindDetail(&dev)
    ));
    #[cfg(feature = "fault-inject")]
    crate::fault_inject::bound(probe.bdf());
    *state.0.lock() = Some(dev);
    if let Err(error) = sys_register_nic_driver() {
        // Dropping the device lets a chip quiesce it on the way out.
        let dev = state.0.lock().take();
        drop(dev);
        let _ = print_fmt(format_args!(
            "{} NIC driver registration failed: {:?}\n",
            D::TAG,
            error
        ));
        sys_exit(1);
    }
}

struct BindDetail<'a, D>(&'a D);

impl<D: NicDevice> core::fmt::Display for BindDetail<'_, D> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.0.bind_detail(f)
    }
}

/// Serve one request from `sender_tid`, replying on the operation (or with a
/// `try_send` for a sender that did not use one).
///
/// Only the live net service may drive the NIC; any other cell could inject or
/// read raw frames. `sender_tid` is kernel-stamped and the registry is asked
/// per request, so a restarted net service is followed without a cached tid.
/// Refusal is the ordinary failure byte the net service retries on.
fn serve<D: NicDevice>(state: &State<D>, sender_tid: usize, data: &[u8]) {
    let operation = ostd::ipc::current();
    let reply = |bytes: &[u8]| {
        if let Some(operation) = operation {
            let _ = ostd::ipc::reply(operation, bytes);
        } else {
            let _ = sys_try_send(sender_tid, bytes);
        }
    };
    if !sender_is_live_provider(service::NET, sender_tid) {
        reply(&[NicStatus::Failed as u8]);
        return;
    }
    let mut out = [0u8; REPLY_CAP];
    match state.0.lock().as_mut() {
        Some(dev) => {
            let len = handle(dev, data, &mut out);
            reply(&out[..len]);
        }
        None => reply(&[NicStatus::Failed as u8]),
    }
}

/// Dispatch one NIC wire request; the reply is `out[..returned]`.
///
/// `OP_TX` is `[op, len u16 LE, frame]`, the length bounding the frame inside
/// the (padded) IPC buffer, and replies one status byte. `OP_RX` replies
/// `[len u16 LE, frame]` (`len == 0`: nothing ready). `OP_GETMAC` replies the
/// six address bytes. Anything else is `Failed`.
fn handle<D: NicDevice>(dev: &mut D, data: &[u8], out: &mut [u8; REPLY_CAP]) -> usize {
    const { assert!(D::MAX_TX <= FRAME_MAX) };
    let status = |out: &mut [u8; REPLY_CAP], status: NicStatus| {
        out[0] = status as u8;
        1
    };
    match data.first() {
        Some(&OP_TX) => {
            if data.len() < 3 {
                return status(out, NicStatus::Failed);
            }
            let len = u16::from_le_bytes([data[1], data[2]]) as usize;
            if len == 0 || len > D::MAX_TX || 3 + len > data.len() {
                return status(out, NicStatus::Failed);
            }
            let frame = &data[3..3 + len];
            #[cfg(feature = "fault-inject")]
            crate::fault_inject::before_tx(D::TAG, frame);
            match dev.send(frame) {
                Ok(()) => status(out, NicStatus::Ok),
                Err(_) => status(out, NicStatus::Failed),
            }
        }
        Some(&OP_RX) => {
            let n = dev.recv(&mut out[2..2 + D::MAX_TX]).min(D::MAX_TX);
            out[..2].copy_from_slice(&(n as u16).to_le_bytes());
            2 + n
        }
        Some(&OP_GETMAC) => {
            out[..6].copy_from_slice(&dev.mac());
            6
        }
        _ => status(out, NicStatus::Failed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use types::{ViError, ViResult};

    /// A NIC that records transmitted frames and returns queued receives.
    struct FakeNic {
        sent: Vec<Vec<u8>>,
        rx: Vec<u8>,
        tx_ok: bool,
    }

    impl NicDevice for FakeNic {
        const TAG: &'static str = "[fake]";
        const VENDOR: u16 = 0;
        const QUERY_IDS: &'static [u16] = &[];
        const ABSENT_LINE: Option<&'static str> = None;
        type Identity = ();
        type Error = ();

        fn classify(_: u16, _: u16) -> Option<()> {
            None
        }
        fn identity_name(_: &()) -> &'static str {
            "fake"
        }
        fn init(_: &Probe, _: ()) -> Result<Self, ()> {
            Err(())
        }
        fn bind_detail(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            f.write_str("detail")
        }
        fn mac(&self) -> [u8; 6] {
            [2, 0, 0, 0, 0, 1]
        }
        fn send(&mut self, frame: &[u8]) -> ViResult<()> {
            self.sent.push(frame.to_vec());
            if self.tx_ok {
                Ok(())
            } else {
                Err(ViError::IO)
            }
        }
        fn recv(&mut self, out: &mut [u8]) -> usize {
            let n = self.rx.len().min(out.len());
            out[..n].copy_from_slice(&self.rx[..n]);
            n
        }
    }

    fn nic() -> FakeNic {
        FakeNic {
            sent: Vec::new(),
            rx: Vec::new(),
            tx_ok: true,
        }
    }

    fn run(dev: &mut FakeNic, data: &[u8]) -> Vec<u8> {
        let mut out = [0u8; REPLY_CAP];
        let len = handle(dev, data, &mut out);
        out[..len].to_vec()
    }

    const OK: u8 = NicStatus::Ok as u8;
    const FAILED: u8 = NicStatus::Failed as u8;

    #[test]
    fn tx_sends_exactly_the_length_prefixed_frame() {
        let mut dev = nic();
        // Padded IPC buffer: bytes past the length header are not sent.
        assert_eq!(
            run(&mut dev, &[OP_TX, 3, 0, 0xA, 0xB, 0xC, 0xFF, 0xFF]),
            [OK]
        );
        assert_eq!(dev.sent, [vec![0xA, 0xB, 0xC]]);
        dev.tx_ok = false;
        assert_eq!(run(&mut dev, &[OP_TX, 1, 0, 7]), [FAILED]);
    }

    #[test]
    fn tx_refuses_empty_short_oversized_and_truncated_frames() {
        let mut dev = nic();
        assert_eq!(run(&mut dev, &[OP_TX]), [FAILED], "no length");
        assert_eq!(run(&mut dev, &[OP_TX, 0, 0]), [FAILED], "zero length");
        assert_eq!(run(&mut dev, &[OP_TX, 4, 0, 1, 2]), [FAILED], "truncated");
        let big = (FRAME_MAX as u16 + 1).to_le_bytes();
        let mut data = vec![OP_TX, big[0], big[1]];
        data.resize(3 + FRAME_MAX + 1, 0);
        assert_eq!(run(&mut dev, &data), [FAILED], "over MAX_TX");
        assert!(dev.sent.is_empty());

        let max = (FRAME_MAX as u16).to_le_bytes();
        let mut data = vec![OP_TX, max[0], max[1]];
        data.resize(3 + FRAME_MAX, 0x5A);
        assert_eq!(run(&mut dev, &data), [OK], "exactly MAX_TX");
    }

    #[test]
    fn rx_replies_length_prefixed_frame_or_zero() {
        let mut dev = nic();
        assert_eq!(run(&mut dev, &[OP_RX]), [0, 0], "nothing ready");
        dev.rx = (0..300u16).map(|i| i as u8).collect();
        let reply = run(&mut dev, &[OP_RX]);
        assert_eq!(&reply[..2], &300u16.to_le_bytes());
        assert_eq!(&reply[2..], &dev.rx[..]);
    }

    #[test]
    fn getmac_replies_six_bytes_and_unknown_ops_fail() {
        let mut dev = nic();
        assert_eq!(run(&mut dev, &[OP_GETMAC]), [2, 0, 0, 0, 0, 1]);
        assert_eq!(run(&mut dev, &[]), [FAILED]);
        assert_eq!(run(&mut dev, &[0xEE]), [FAILED]);
    }
}
