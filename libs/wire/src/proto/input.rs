// SPDX-License-Identifier: Apache-2.0
//! Input wires: producer frames into the input service, the event delivered
//! to the focused cell, and input → producer control frames.
//!
//! Producer frame: `[kind][code u32 LE][value i32 LE]`, 9 B, from the kernel
//! (sender 0), the UART relay or a registered producer. `EV_DEVICE` frames
//! carry a device id first: `[EV_DEVICE][device u32][kind][code u32][value i32]`.
//! These `EV_*` kinds are not the Linux evdev types of
//! [`crate::vmm_session`] (there `EV_KEY` is 1). The kernel emits producer
//! frames itself, so their kinds and lengths live in `abi::input_producer`.

pub use crate::input::INPUT_EVENT_OPCODE;
pub use crate::ipc::{input_source, led_bits, OP_HID_DEVICE_REMOVED, OP_SET_LEDS};

pub use abi::input_producer::{
    DEVICE_FRAME_LEN, EV_ABS, EV_ASCII, EV_DEVICE, EV_KEY, EV_REL, PRODUCER_FRAME_LEN,
};
