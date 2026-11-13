// SPDX-License-Identifier: Apache-2.0
//! Net service ↔ NIC driver wire (little-endian).
//!
//! - `OP_TX`: `[OP_TX, len u16][frame]` → `[NicStatus]`. The length is
//!   required: raw IPC hands the receiver its whole buffer with no count.
//! - `OP_RX`: `[OP_RX]` → `[len u16][frame]`, len 0 = nothing ready; a reply
//!   shorter than [`MIN_FRAME`] is a [`NicStatus`].
//! - `OP_GETMAC`: `[OP_GETMAC]` → 6 MAC bytes.

pub const OP_TX: u8 = 0;
pub const OP_RX: u8 = 1;
pub const OP_GETMAC: u8 = 2;

pub const TX_HEADER_LEN: usize = 3;
pub const RX_HEADER_LEN: usize = 2;
pub const MAC_LEN: usize = 6;
/// Shortest Ethernet frame; shorter replies are statuses.
pub const MIN_FRAME: usize = 14;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum NicStatus {
    Ok = 0,
    Failed = 1,
    NotReady = 2,
}
