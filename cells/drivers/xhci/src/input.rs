//! Input-service producer.
//!
//! Decoded HID events travel through the **same** path the BCM DWC2 USB driver
//! uses: register once as a raw event source with the input service, then send
//! `EV_DEVICE` frames. No second input stack.
//!
//! The input service accepts `USB_HID_HOST` only from the tid the kernel has
//! published as `service::USB_HID_PRODUCER` (see `cells/services/input/src/main.rs`),
//! so the cell calls `sys_register_usb_hid_producer()` first to acquire that
//! kernel-verified producer identity. It deliberately does **not** call
//! `sys_register_nic_driver()`: that role is the singleton NIC owner and
//! claiming it would redirect network IPC to this cell.

use api::ipc::{self, input_source, InputRequest};
use driver_hid::{EvdevEvent, HidDeviceId, DEVICE_EVENT_LEN};
use ostd::syscall::{sys_send, SyscallResult};

/// Register this cell as a raw event producer with the input service.
///
/// This must be a queued `Send`, the same delivery class as the events it
/// gates. The input service is a polling owner loop (nonblocking receive plus a
/// one-tick sleep) that never parks in `Recv`, and the kernel delivers a
/// `TrySend` only to a target parked in `Recv` (or from the input service and
/// compositor themselves), so a `TrySend` registration is refused every time
/// and no decoded key ever reaches the input service. `Send` queues the request
/// in the service's mailbox and returns once the service has taken it.
pub fn register_as_source(tid: usize) -> bool {
    if tid == 0 {
        return false;
    }
    let mut buf = [0u8; 32];
    match ipc::encode(
        &InputRequest::RegisterEventSource {
            kind: input_source::USB_HID_HOST,
        },
        &mut buf,
    ) {
        Ok(encoded) => matches!(sys_send(tid, encoded), SyscallResult::Ok(_)),
        Err(_) => false,
    }
}

/// Send one device-identified event to the input service.
pub fn forward_device_event(tid: usize, device: HidDeviceId, ev: &EvdevEvent) -> bool {
    if tid == 0 {
        return false;
    }
    let mut buf = [0u8; DEVICE_EVENT_LEN];
    ev.encode_device(device, &mut buf);
    matches!(sys_send(tid, &buf), SyscallResult::Ok(_))
}
