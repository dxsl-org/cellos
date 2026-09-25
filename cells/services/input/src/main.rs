// The layout and lock-key logic is pure and is unit-tested on the host, where
// `no_std`/`no_main` would only get in the way (same arrangement as
// `cells/services/httpd`).
#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
// The crate cannot carry `#![forbid(unsafe_code)]`: `virtio_device.rs` needs MMIO
// access (allowlisted, class driver-mmio). All other submodules are unsafe-free.

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

use alloc::vec::Vec;
use api::input::{InputEvent, KeyEvent, KeyState, KeySym};
use api::ipc::{InputRequest, InputResponse, IPC_BUF_SIZE, OP_HID_DEVICE_REMOVED};
use api::syscall::service;
use dispatcher::Dispatcher;
use layout_us_qwerty::{key_state_from_evdev, modifier_for_scancode, translate};
use modifier_state::ModifierState;
use mouse_state::{btn_to_mouse_button, MouseState, BTN_LEFT};
use ostd::io::{print, print_usize, println};
use ostd::syscall::{
    sys_get_time, sys_heartbeat, sys_lookup_service, sys_recv_timeout, sys_try_send, SyscallResult,
};
use virtio_device::{find_and_init_inputs, InputDevice};
api::declare_manifest!(block_io = false, network = false, spawn = false);
// LookupService: the dispatcher resolves the compositor TID for mouse routing.
api::declare_syscalls![
    Send,
    TrySend,
    Recv,
    RecvTimeout,
    Log,
    Heartbeat,
    GetTime,
    RequestMmio,
    GrantAlloc,
    GrantFree,
    LookupService
];

/// Raw event type discriminant for keyboard events (kernel VirtIO push).
const EV_KEY: u8 = 0;
/// Raw event type for UART ASCII relay from the kernel console driver.
/// The code field carries the raw ASCII byte; no scancode translation needed.
const EV_ASCII: u8 = 0x04;
/// A per-interface HID worker event carrying a logical-device ID.
///
/// Layout: `[EV_DEVICE][device:u32][event_type][code:u32][value:i32]`.
const EV_DEVICE: u8 = 0x05;

/// Linux evdev event types used by VirtIO input device.
const EVDEV_KEY: u16 = 1;
const EVDEV_REL: u16 = 2;
const EVDEV_ABS: u16 = 3;

/// Poll the VirtIO virtqueue once per scheduler tick (≈10 ms).
/// Using scheduler-tick units (not mtime); see net service UNIT TRAP note.
const POLL_SCHED_TICKS: u64 = 1;

/// Watchdog interval: 5000 scheduler ticks × 10ms = 50 seconds.
/// Must match DISPATCH_HEARTBEAT in dispatcher.rs — they share the same timeline.
const HEARTBEAT_TICKS: u64 = 5_000;

/// Input Cell entry point.
///
/// On startup, attempts to probe and claim a VirtIO input device. Once claimed,
/// the kernel's `virtio_input::poll_events` / `dispatch_pending` migrate guard
/// detects the MMIO owner and stops pushing kernel-side events — this service
/// then owns the virtqueue exclusively.
///
/// Until the device is claimed (or if no VirtIO input is present), the kernel
/// continues to push raw IPC events (sender=0) as before.
#[cfg(not(test))]
#[no_mangle]
pub fn main() {
    println("[input] Input Service v0.3: US QWERTY + VirtIO + typed focus routing");

    let mut modifiers = ModifierState::new();
    let mut mouse = MouseState::new();
    let mut dispatcher = Dispatcher::new();
    let mut buf = [0u8; IPC_BUF_SIZE];
    // Last LED bitmap handed to the producers, so a lock key that is pressed and
    // released without changing a lock costs nothing.
    let mut leds_sent = modifiers.led_bitmap();

    // Renew the watchdog before the potentially slow VirtIO MMIO probe — the
    // cell is spawned with a default deadline and find_and_init_input can take
    // several scheduling cycles before we reach the loop's sys_heartbeat call.
    sys_heartbeat(HEARTBEAT_TICKS);

    // Probe and claim ALL VirtIO input devices (QEMU exposes keyboard, tablet,
    // mouse as separate virtio-input MMIO slots).  After sys_request_mmio
    // succeeds inside find_and_init_inputs, the kernel migration guard in
    // virtio_input.rs sees the MMIO owner and stops pushing events via
    // dispatch_pending — so every unclaimed device would be polled by nobody.
    let mut sources = EventSources::new();
    let mut hid_devices = HidDeviceStates::new();
    let mut devices: Vec<InputDevice> = find_and_init_inputs();
    if !devices.is_empty() {
        println("[input] VirtIO input device claimed; polling virtqueue directly");
    } else {
        println("[input] No VirtIO input device; relying on kernel push");
    }

    loop {
        sys_heartbeat(HEARTBEAT_TICKS);

        // Drain every VirtIO virtqueue before blocking.  This catches events
        // that arrived since the last iteration without waiting for the timeout.
        for dev in devices.iter_mut() {
            drain_virtio(
                dev,
                &mut buf,
                &mut modifiers,
                &mut mouse,
                &mut dispatcher,
                &mut hid_devices,
            );
        }

        // Block for at most one scheduler tick (≈10ms), or until a kernel/IPC
        // message wakes us.
        //
        // Return value convention:
        //   Ok(0)                — real timeout; buffer not modified
        //   Ok(isize::MAX as _)  — kernel UART relay (EV_ASCII); buffer filled
        //   Ok(n)                — typed IPC from cell n; buffer filled
        //
        // The sentinel is isize::MAX (not usize::MAX) because syscall() returns
        // isize: usize::MAX == -1 as isize, which makes sys_recv_timeout return
        // Err and the match arm never fires. isize::MAX is positive → Ok branch.
        match sys_recv_timeout(0, &mut buf, POLL_SCHED_TICKS) {
            SyscallResult::Ok(0) => {
                // Real timeout — nothing from IPC; VirtIO drain already done above.
            }
            SyscallResult::Ok(n) if n == isize::MAX as usize => {
                // Kernel UART relay (sentinel sender_id = isize::MAX as usize).
                handle_kernel_event(
                    &buf,
                    &mut modifiers,
                    &mut mouse,
                    &mut dispatcher,
                    &mut hid_devices,
                );
            }
            SyscallResult::Ok(sender) => {
                handle_message(
                    &buf,
                    sender,
                    &mut modifiers,
                    &mut mouse,
                    &mut dispatcher,
                    &mut sources,
                    &mut hid_devices,
                );
            }
            _ => {}
        }

        // Lock keys change what the keyboard's own LEDs show, and the device is
        // the only place a user can see that state. Producers hear about it once
        // per change rather than once per key, and a producer that comes up
        // later is told at registration.
        let leds = modifiers.led_bitmap();
        if leds != leds_sent {
            sources.broadcast_leds(leds);
            leds_sent = leds;
        }
    }
}

/// Registered raw-event producers. The DWC2 host is the sole USB producer:
/// isolated HID workers return decoded frames to that host instead of acquiring
/// a second input-service identity.
const MAX_EVENT_SOURCES: usize = 16;

/// TIDs allowed to push raw `[opcode][code][value]` events.
///
/// Sender 0 is the kernel and is implicit. For DWC2, Input accepts only the
/// kernel's current NIC-driver endpoint. The recorded identity always comes
/// from receive metadata, never from a payload.
struct EventSources {
    tids: [usize; MAX_EVENT_SOURCES],
    kinds: [u8; MAX_EVENT_SOURCES],
    len: usize,
}

impl EventSources {
    const fn new() -> Self {
        Self {
            tids: [0; MAX_EVENT_SOURCES],
            kinds: [0; MAX_EVENT_SOURCES],
            len: 0,
        }
    }

    fn is_source(&self, tid: usize) -> bool {
        tid != 0 && self.tids[..self.len].contains(&tid)
    }

    fn is_hid_host(&self, tid: usize) -> bool {
        self.tids[..self.len]
            .iter()
            .zip(self.kinds[..self.len].iter())
            .any(|(&source, &kind)| source == tid && kind == api::ipc::input_source::USB_HID_HOST)
    }

    /// Record the kernel-verified DWC2 host as the USB producer.
    fn register(&mut self, tid: usize, kind: u8) -> bool {
        if let Some(index) = self.tids[..self.len]
            .iter()
            .position(|&source| source == tid)
        {
            return self.kinds[index] == kind;
        }
        if tid == 0
            || kind != api::ipc::input_source::USB_HID_HOST
            || sys_lookup_service(service::NIC_DRIVER) != Some(tid)
            || self.len == MAX_EVENT_SOURCES
        {
            return false;
        }
        self.tids[self.len] = tid;
        self.kinds[self.len] = kind;
        self.len += 1;
        true
    }

    /// Tell one producer what the lock LEDs should show.
    fn send_leds(&self, tid: usize, leds: u8) {
        let frame = [api::ipc::OP_SET_LEDS, leds];
        if !matches!(sys_try_send(tid, &frame), SyscallResult::Ok(0)) {
            print("[input] WARN: lock LED state not delivered to source tid=");
            print_usize(tid);
            println("");
        }
    }

    /// Tell every registered producer what the lock LEDs should show.
    fn broadcast_leds(&self, leds: u8) {
        for &tid in &self.tids[..self.len] {
            self.send_leds(tid, leds);
        }
    }
}

/// Pressed-key ownership for isolated HID interfaces.
///
/// This is deliberately bounded: an untrusted descriptor/report cannot grow
/// input-service state without limit, and disconnect cleanup only touches the
/// keys that one device actually contributed.
const MAX_HID_DEVICES: usize = 16;
const MAX_KEYS_PER_HID_DEVICE: usize = 128;

struct HidDeviceState {
    id: u32,
    pressed: Vec<u32>,
}

struct HidDeviceStates {
    devices: Vec<HidDeviceState>,
}

impl HidDeviceStates {
    const fn new() -> Self {
        Self {
            devices: Vec::new(),
        }
    }

    fn record_key(&mut self, id: u32, code: u32, state: KeyState) {
        let index = match self.devices.iter().position(|device| device.id == id) {
            Some(index) => index,
            None if self.devices.len() < MAX_HID_DEVICES => {
                self.devices.push(HidDeviceState {
                    id,
                    pressed: Vec::new(),
                });
                self.devices.len() - 1
            }
            None => return,
        };
        let pressed = &mut self.devices[index].pressed;
        match state {
            KeyState::Pressed | KeyState::Repeated => {
                if !pressed.contains(&code) && pressed.len() < MAX_KEYS_PER_HID_DEVICE {
                    pressed.push(code);
                }
            }
            KeyState::Released => pressed.retain(|&held| held != code),
        }
    }

    fn take_pressed(&mut self, id: u32) -> Vec<u32> {
        let Some(index) = self.devices.iter().position(|device| device.id == id) else {
            return Vec::new();
        };
        self.devices.swap_remove(index).pressed
    }

    fn has_transient_modifier(&self, code: u32) -> bool {
        let Some(modifier) = modifier_for_scancode(code) else {
            return false;
        };
        self.devices.iter().any(|device| {
            device
                .pressed
                .iter()
                .copied()
                .any(|held| modifier_for_scancode(held) == Some(modifier))
        })
    }
}

fn release_hid_device(
    device: u32,
    modifiers: &mut ModifierState,
    mouse: &mut MouseState,
    dispatcher: &mut Dispatcher,
    hid_devices: &mut HidDeviceStates,
) {
    for code in hid_devices.take_pressed(device) {
        // A second keyboard can keep the same logical modifier held (including
        // left/right variants). Do not clear its aggregate state on this
        // worker's teardown.
        if hid_devices.has_transient_modifier(code) {
            continue;
        }
        let mut release = [0u8; IPC_BUF_SIZE];
        release[0] = EV_KEY;
        release[1..5].copy_from_slice(&code.to_le_bytes());
        // value remains zero: EV_KEY release.
        handle_kernel_event(&release, modifiers, mouse, dispatcher, hid_devices);
    }
}

/// Drain all pending events from the VirtIO virtqueue and dispatch them.
fn drain_virtio(
    dev: &mut InputDevice,
    buf: &mut [u8; IPC_BUF_SIZE],
    modifiers: &mut ModifierState,
    mouse: &mut MouseState,
    dispatcher: &mut Dispatcher,
    hid_devices: &mut HidDeviceStates,
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
        handle_kernel_event(buf, modifiers, mouse, dispatcher, hid_devices);
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
    sources: &mut EventSources,
    hid_devices: &mut HidDeviceStates,
) {
    // Registered producers speak raw frames; all other senders are typed IPC.
    if sender == 0 {
        handle_kernel_event(buf, modifiers, mouse, dispatcher, hid_devices);
    } else if sources.is_source(sender) {
        if buf[0] == OP_HID_DEVICE_REMOVED && sources.is_hid_host(sender) {
            let device = u32::from_le_bytes([buf[1], buf[2], buf[3], buf[4]]);
            release_hid_device(device, modifiers, mouse, dispatcher, hid_devices);
            return;
        }
        handle_kernel_event(buf, modifiers, mouse, dispatcher, hid_devices);
    } else {
        handle_typed_request(buf, sender, modifiers, dispatcher, sources);
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
    hid_devices: &mut HidDeviceStates,
) {
    let (device, opcode, code, value) = if buf[0] == EV_DEVICE {
        if buf.len() < 14 {
            return;
        }
        (
            Some(u32::from_le_bytes([buf[1], buf[2], buf[3], buf[4]])),
            buf[5],
            u32::from_le_bytes([buf[6], buf[7], buf[8], buf[9]]),
            u32::from_le_bytes([buf[10], buf[11], buf[12], buf[13]]),
        )
    } else {
        if buf.len() < 9 {
            return;
        }
        (
            None,
            buf[0],
            u32::from_le_bytes([buf[1], buf[2], buf[3], buf[4]]),
            u32::from_le_bytes([buf[5], buf[6], buf[7], buf[8]]),
        )
    };

    match opcode {
        EV_KEY => {
            let state = key_state_from_evdev(value);
            if let Some(device) = device {
                hid_devices.record_key(device, code, state);
            }
            // BTN_* codes (≥ 0x110) are mouse buttons, not keyboard keys.
            // Pointer events route to the compositor (cursor + Z-order owner),
            // not the keyboard-focused cell — see Dispatcher::dispatch_mouse.
            if code >= BTN_LEFT {
                if let Some(button) = btn_to_mouse_button(code) {
                    dispatcher.dispatch_mouse(&InputEvent::MouseButton { button, state });
                }
                return;
            }
            if modifiers.update(code, state) {
                return;
            }
            let (keysym, character) = translate(code, modifiers.snapshot());
            if !dispatcher.dispatch(&InputEvent::Key(KeyEvent {
                timestamp_ticks: sys_get_time(),
                scancode: code,
                keysym,
                character,
                modifiers: modifiers.snapshot(),
                state,
                _pad: [0; 2],
            })) {
                println("[input] WARNING: clearing failed keyboard focus");
            }
        }
        1 => {
            if let Some(ev) = mouse.apply_rel(code, value) {
                dispatcher.dispatch_mouse(&ev);
            }
        }
        2 => {
            if let Some(ev) = mouse.apply_abs(code, value) {
                dispatcher.dispatch_mouse(&ev);
            }
        }
        EV_ASCII => {
            // UART byte relayed by the kernel console driver.
            // `code` carries the raw ASCII code point; skip scancode translation.
            // Map C0 control chars to semantic KeySyms so GUI apps get proper events
            // regardless of whether input originates from VirtIO or UART terminal.
            let state = if value > 0 {
                KeyState::Pressed
            } else {
                KeyState::Released
            };
            let (keysym, character) = match code {
                0x1B => (KeySym::Escape, 0),
                0x0D | 0x0A => (KeySym::Return, code),
                0x08 | 0x7F => (KeySym::Backspace, code),
                0x09 => (KeySym::Tab, code),
                _ => (KeySym::Printable, code),
            };
            if !dispatcher.dispatch(&InputEvent::Key(KeyEvent {
                timestamp_ticks: sys_get_time(),
                scancode: 0,
                keysym,
                character,
                modifiers: modifiers.snapshot(),
                state,
                _pad: [0; 2],
            })) {
                println("[input] WARNING: clearing failed keyboard focus");
            }
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
    sources: &mut EventSources,
) {
    let mut resp_buf = [0u8; 64];
    match api::ipc::decode::<InputRequest>(buf) {
        Ok(InputRequest::SetFocus { cell_tid: _ }) => {
            modifiers.reset_transient();
            // Use kernel-verified sender TID instead of the cell_tid field to
            // prevent a cell from redirecting focus to an arbitrary TID.
            dispatcher.set_focus(sender);
            // Fire-and-forget: no reply. Focus is set atomically on receipt.
            // A blocking reply would deadlock when the focused cell is not yet
            // in sys_recv (startup race — G18 deadlock fix).
        }
        Ok(InputRequest::GetFocus) => {
            let focused = dispatcher.focus() as u32;
            if let Ok(encoded) = api::ipc::encode(&InputResponse::Focus(focused), &mut resp_buf) {
                // GetFocus is only called by compositor (never during startup race).
                // Use sys_try_send to be safe — compositor is in recv waiting for this.
                let _ = sys_try_send(sender, encoded);
            }
        }
        Ok(InputRequest::ClearFocus { cell_tid: _ }) => {
            // Use kernel-verified sender TID (same as SetFocus) — prevents a cell
            // from clearing another cell's focus. When sender == focused, drop focus.
            if dispatcher.focus() == sender {
                dispatcher.set_focus(0);
            }
            // Fire-and-forget: no reply. Same rationale as SetFocus.
        }
        Ok(InputRequest::RegisterEventSource { kind }) => {
            if sources.register(sender, kind) {
                print("[input] registered raw event source kind=");
                print_usize(kind as usize);
                println("");
                // A producer that just came up does not know what the locks are
                // set to, and its keyboard would otherwise show stale state.
                sources.send_leds(sender, modifiers.led_bitmap());
            } else {
                println("[input] WARN: event-source registration refused");
            }
        }
        Err(_) => {} // unknown message — drop silently
    }
}

#[cfg(test)]
mod hid_device_tests {
    use super::HidDeviceStates;
    use api::input::KeyState;

    #[test]
    fn disconnect_releases_only_its_own_pressed_keys() {
        let mut devices = HidDeviceStates::new();
        devices.record_key(1, 0x1E, KeyState::Pressed);
        devices.record_key(2, 0x30, KeyState::Pressed);
        devices.record_key(1, 0x1E, KeyState::Released);
        devices.record_key(1, 0x2E, KeyState::Pressed);

        assert_eq!(devices.take_pressed(1), [0x2E]);
        assert_eq!(devices.take_pressed(2), [0x30]);
    }
}
