#![no_std]
#![no_main]
// Note: #[no_mangle] on main() is required by the ViCell ELF loader and triggers
// unsafe_attr, so we cannot use #![forbid(unsafe_code)] globally here.
// All business logic in the submodules is unsafe-free.

//! Input Service Cell.
//!
//! Receives raw EV_KEY events from the kernel VirtIO input driver via IPC,
//! translates scancodes to `InputEvent`s using the US QWERTY layout, and
//! dispatches them to the currently focused cell.
//!
//! ## IPC protocol (inbound from kernel, sender == 0)
//! ```text
//! byte[0]   = event type: 0=EV_KEY, 1=EV_REL, 2=EV_ABS
//! byte[1..5]= code  (u32 LE: scancode, REL_*, ABS_* axis)
//! byte[5..9]= value (u32 LE: key state, signed rel delta, abs coord)
//! ```
//! Sender 0 is the kernel; these raw frames bypass postcard decoding entirely.
//!
//! ## Focus IPC (inbound from compositor/shell, sender > 0)
//! Typed `InputRequest` encoded with postcard — see `api::ipc::InputRequest`.
//! Sender > 0 always routes to postcard decode; opcode collisions with kernel
//! frames are impossible by construction.
//!
//! ## IPC protocol (outbound to focused cell)
//! See `dispatcher::Dispatcher::dispatch` and `api::input::encode_event`.

extern crate alloc;

mod dispatcher;
mod layout_us_qwerty;
mod modifier_state;
mod mouse_state;
mod virtio_device;

use api::input::{InputEvent, KeyEvent, KeyState, KeySym, Modifiers};
use api::ipc::{InputRequest, InputResponse, IPC_BUF_SIZE};
use dispatcher::Dispatcher;
use layout_us_qwerty::{translate, key_state_from_evdev};
use modifier_state::ModifierState;
use mouse_state::{MouseState, btn_to_mouse_button, BTN_LEFT};
use ostd::io::println;
use ostd::syscall::{sys_recv_timeout, sys_send, sys_get_time, sys_heartbeat, SyscallResult};
use virtio_device::{find_and_init_input, InputDevice};

api::declare_manifest!(block_io = false, network = false, spawn = false);
api::declare_syscalls![Send, Recv, RecvTimeout, Log, Heartbeat, GetTime, RequestMmio];

/// Raw event type discriminant for keyboard events (kernel VirtIO push).
const EV_KEY: u8 = 0;
/// Raw event type for UART ASCII relay from the kernel console driver.
/// The code field carries the raw ASCII byte; no scancode translation needed.
const EV_ASCII: u8 = 0x04;

/// Linux evdev event types used by VirtIO input device.
const EVDEV_KEY: u16 = 1;
const EVDEV_REL: u16 = 2;
const EVDEV_ABS: u16 = 3;

/// Poll the VirtIO virtqueue once per scheduler tick (≈10 ms).
/// Using scheduler-tick units (not mtime); see net service UNIT TRAP note.
const POLL_SCHED_TICKS: u64 = 1;

/// Watchdog interval: 500 scheduler ticks × 10ms = 5 seconds.
const HEARTBEAT_TICKS: u64 = 500;

/// Input Cell entry point.
///
/// On startup, attempts to probe and claim a VirtIO input device. Once claimed,
/// the kernel's `virtio_input::poll_events` / `dispatch_pending` migrate guard
/// detects the MMIO owner and stops pushing kernel-side events — this service
/// then owns the virtqueue exclusively.
///
/// Until the device is claimed (or if no VirtIO input is present), the kernel
/// continues to push raw IPC events (sender=0) as before.
#[no_mangle]
pub fn main() {
    println("[input] Input Service v0.3: US QWERTY + VirtIO + typed focus routing");

    let mut modifiers = ModifierState::new();
    let mut mouse = MouseState::new();
    let mut dispatcher = Dispatcher::new();
    let mut buf = [0u8; IPC_BUF_SIZE];

    // Probe and claim the VirtIO input device.  After sys_request_mmio succeeds
    // inside find_and_init_input, the kernel migration guard in virtio_input.rs
    // will see the MMIO owner and stop pushing events via dispatch_pending.
    let mut device: Option<InputDevice> = find_and_init_input();
    if device.is_some() {
        println("[input] VirtIO input device claimed; polling virtqueue directly");
    } else {
        println("[input] No VirtIO input device; relying on kernel push");
    }

    loop {
        sys_heartbeat(HEARTBEAT_TICKS);

        // Drain the VirtIO virtqueue before blocking.  This catches events that
        // arrived since the last iteration without waiting for the timeout.
        if let Some(ref mut dev) = device {
            drain_virtio(dev, &mut buf, &mut modifiers, &mut mouse, &mut dispatcher);
        }

        // Block for at most one scheduler tick (≈10ms), or until a kernel/IPC
        // message wakes us.
        match sys_recv_timeout(0, &mut buf, POLL_SCHED_TICKS) {
            SyscallResult::Ok(0) => {
                // Timeout — nothing from IPC; VirtIO drain already done above.
            }
            SyscallResult::Ok(sender) => {
                handle_message(&buf, sender, &mut modifiers, &mut mouse, &mut dispatcher);
            }
            _ => {}
        }
    }
}

/// Drain all pending events from the VirtIO virtqueue and dispatch them.
fn drain_virtio(
    dev: &mut InputDevice,
    buf: &mut [u8; IPC_BUF_SIZE],
    modifiers: &mut ModifierState,
    mouse: &mut MouseState,
    dispatcher: &mut Dispatcher,
) {
    while let Some(ev) = dev.try_get_event() {
        // Map Linux evdev event types → the same opcode encoding the kernel uses
        // in dispatch_pending, so handle_kernel_event processes them identically.
        let opcode: u8 = match ev.event_type {
            EVDEV_KEY => 0, // EV_KEY
            EVDEV_REL => 1, // EV_REL
            EVDEV_ABS => 2, // EV_ABS
            _ => continue,  // unknown type (EV_SYN, EV_MSC, etc.) — drop
        };
        buf[0] = opcode;
        buf[1..5].copy_from_slice(&(ev.code as u32).to_le_bytes());
        buf[5..9].copy_from_slice(&ev.value.to_le_bytes());
        handle_kernel_event(buf, modifiers, mouse, dispatcher);
    }
}

/// Process one incoming IPC message.
///
/// Discrimination is by `sender`, not opcode, to avoid collisions with postcard
/// discriminants: kernel pushes arrive with sender=0; typed requests sender>0.
fn handle_message(
    buf: &[u8; IPC_BUF_SIZE],
    sender: usize,
    modifiers: &mut ModifierState,
    mouse: &mut MouseState,
    dispatcher: &mut Dispatcher,
) {
    if sender == 0 {
        handle_kernel_event(buf, modifiers, mouse, dispatcher);
    } else {
        handle_typed_request(buf, sender, modifiers, dispatcher);
    }
}

/// Handle a raw VirtIO event pushed by the kernel (sender == 0).
///
/// Wire format: `[opcode:1][code:4 LE][value:4 LE]`
/// opcode 0 = EV_KEY (keyboard key or mouse button via BTN_* scancode ≥ 0x110)
/// opcode 1 = EV_REL (relative mouse: REL_X/Y/WHEEL)
/// opcode 2 = EV_ABS (absolute mouse: ABS_X/Y)
fn handle_kernel_event(
    buf: &[u8; IPC_BUF_SIZE],
    modifiers: &mut ModifierState,
    mouse: &mut MouseState,
    dispatcher: &mut Dispatcher,
) {
    if buf.len() < 9 { return; }
    let code  = u32::from_le_bytes([buf[1], buf[2], buf[3], buf[4]]);
    let value = u32::from_le_bytes([buf[5], buf[6], buf[7], buf[8]]);

    match buf[0] {
        EV_KEY => {
            let state = key_state_from_evdev(value);
            // BTN_* codes (≥ 0x110) are mouse buttons, not keyboard keys.
            if code >= BTN_LEFT {
                if let Some(button) = btn_to_mouse_button(code) {
                    dispatcher.dispatch(&InputEvent::MouseButton { button, state });
                }
                return;
            }
            if modifiers.update(code, state) { return; }
            let (keysym, character) = translate(code, modifiers.snapshot());
            dispatcher.dispatch(&InputEvent::Key(KeyEvent {
                timestamp_ticks: sys_get_time(),
                scancode: code,
                keysym,
                character,
                modifiers: modifiers.snapshot(),
                state,
                _pad: [0; 2],
            }));
        }
        1 => {
            if let Some(ev) = mouse.apply_rel(code, value) {
                dispatcher.dispatch(&ev);
            }
        }
        2 => {
            if let Some(ev) = mouse.apply_abs(code, value) {
                dispatcher.dispatch(&ev);
            }
        }
        EV_ASCII => {
            // UART byte relayed by the kernel console driver.
            // `code` carries the raw ASCII code point; skip scancode translation.
            // Map C0 control chars to semantic KeySyms so GUI apps get proper events
            // regardless of whether input originates from VirtIO or UART terminal.
            let state = if value > 0 { KeyState::Pressed } else { KeyState::Released };
            let (keysym, character) = match code {
                0x1B        => (KeySym::Escape,    0),
                0x0D | 0x0A => (KeySym::Return,    code),
                0x08 | 0x7F => (KeySym::Backspace, code),
                0x09        => (KeySym::Tab,        code),
                _           => (KeySym::Printable,  code),
            };
            dispatcher.dispatch(&InputEvent::Key(KeyEvent {
                timestamp_ticks: sys_get_time(),
                scancode: 0,
                keysym,
                character,
                modifiers: modifiers.snapshot(),
                state,
                _pad: [0; 2],
            }));
        }
        _ => {} // unknown opcode — drop silently
    }
}

/// Handle a typed `InputRequest` from a compositor or shell cell (sender > 0).
fn handle_typed_request(
    buf: &[u8; IPC_BUF_SIZE],
    sender: usize,
    modifiers: &mut ModifierState,
    dispatcher: &mut Dispatcher,
) {
    let mut resp_buf = [0u8; 64];
    match api::ipc::decode::<InputRequest>(buf) {
        Ok(InputRequest::SetFocus { cell_tid: _ }) => {
            modifiers.reset_transient();
            // Use kernel-verified sender TID instead of the cell_tid field to
            // prevent a cell from redirecting focus to an arbitrary TID.
            dispatcher.set_focus(sender);
            if let Ok(encoded) = api::ipc::encode(&InputResponse::Ok, &mut resp_buf) {
                sys_send(sender, encoded);
            }
        }
        Ok(InputRequest::GetFocus) => {
            let focused = dispatcher.focus() as u32;
            if let Ok(encoded) = api::ipc::encode(&InputResponse::Focus(focused), &mut resp_buf) {
                sys_send(sender, encoded);
            }
        }
        Ok(InputRequest::ClearFocus { cell_tid }) => {
            if dispatcher.focus() == cell_tid as usize {
                dispatcher.set_focus(0);
            }
            if let Ok(encoded) = api::ipc::encode(&InputResponse::Ok, &mut resp_buf) {
                sys_send(sender, encoded);
            }
        }
        Err(_) => {} // unknown message — drop silently
    }
}
