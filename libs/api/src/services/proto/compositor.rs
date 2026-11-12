// SPDX-License-Identifier: Apache-2.0
//! Compositor wire. Opcodes, events and frame layouts are owned by
//! [`crate::services::display`]; this module adds the bytes that were literals.

pub use super::supervisor::{OWNER_EXITED_LEN, OWNER_EXITED_RELAY};
pub use crate::services::display::{compositor_events, compositor_ops, COMPOSITOR_ENDPOINT};

/// One-byte boolean reply of the grant and surface requests.
pub const REPLY_FALSE: u8 = 0;
pub const REPLY_TRUE: u8 = 1;

/// `DESTROY_SURFACE` status reply; note it is not the boolean above.
pub const DESTROY_OK: u8 = 0;
/// Unknown surface, or the caller does not own it.
pub const DESTROY_REFUSED: u8 = 1;
