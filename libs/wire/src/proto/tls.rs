// SPDX-License-Identifier: Apache-2.0
//! Raw TLS frames to the net service. Every request starts `[op][cap u64 LE]`.
//!
//! - `CONNECT`: `[op][cap=0][ip 4][port u16][hostname_len u16][hostname]` →
//!   reply `cap u64`, [`CONNECT_FAILED`] on failure.
//! - `SEND`: `[op][cap][len u16][data]` → reply `u32` count, [`SEND_ERROR`].
//! - `RECV`: `[op][cap][want u32]` → reply `[len u16][data]`; len 0 is EOF,
//!   [`RECV_ERROR`] an error.
//! - `CLOSE`: `[op][cap]` → `[CLOSE_OK]` or `[CLOSE_FAILED]`; an empty reply
//!   is a parse error.

pub const CLOSE: u8 = 0x15;
pub const CONNECT: u8 = 0x30;
pub const SEND: u8 = 0x31;
pub const RECV: u8 = 0x32;

pub const HEADER_LEN: usize = 9;
pub const CONNECT_HEADER_LEN: usize = 17;
pub const SEND_HEADER_LEN: usize = 11;
pub const RECV_REQUEST_LEN: usize = 13;
pub const MAX_HOSTNAME: usize = 495;
pub const MAX_SEND: usize = 501;
pub const MAX_RECV_DATA: usize = 4094;

pub const CONNECT_FAILED: u64 = 0;
pub const SEND_ERROR: u32 = u32::MAX;
pub const RECV_EOF: u16 = 0;
pub const RECV_ERROR: u16 = u16::MAX;
pub const CLOSE_OK: u8 = 0x00;
pub const CLOSE_FAILED: u8 = 0xFF;
