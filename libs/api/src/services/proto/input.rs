// SPDX-License-Identifier: Apache-2.0
//! Input wires: producer frames into the input service, the event delivered
//! to the focused cell, and input → producer control frames.
//!
//! Producer frame: `[kind][code u32 LE][value i32 LE]`, 9 B, from the kernel
//! (sender 0), the UART relay or a registered producer. `EV_DEVICE` frames
//! carry a device id first: `[EV_DEVICE][device u32][kind][code u32][value i32]`.
//! These `EV_*` kinds are not the Linux evdev types of
//! [`crate::services::vmm_session`] (there `EV_KEY` is 1).

pub use crate::services::input::INPUT_EVENT_OPCODE;
pub use crate::services::ipc::{input_source, led_bits, OP_HID_DEVICE_REMOVED, OP_SET_LEDS};

pub const EV_KEY: u8 = 0;
pub const EV_REL: u8 = 1;
pub const EV_ABS: u8 = 2;
/// UART relay; the code is the ASCII byte.
pub const EV_ASCII: u8 = 0x04;
pub const EV_DEVICE: u8 = 0x05;

pub const PRODUCER_FRAME_LEN: usize = 9;
pub const DEVICE_FRAME_LEN: usize = 14;
