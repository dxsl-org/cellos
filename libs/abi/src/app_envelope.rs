// SPDX-License-Identifier: Apache-2.0
//! App SDK envelope: `[MAGIC, kind, …]`, the frame `ostd::app::AppContext`
//! decodes and the supervisor, kernel and hot-swap tools send.

/// Byte 0 of every envelope frame.
pub const MAGIC: u8 = 0xAC;
/// Envelope header length: magic plus kind.
pub const HEADER_LEN: usize = 2;

/// Byte 1: an application message; the payload follows the header.
pub const KIND_MESSAGE: u8 = 0x00;
/// Byte 1: hot-swap snapshot request, `[MAGIC, KIND_SNAPSHOT, swap_id u64 LE]`.
pub const KIND_SNAPSHOT: u8 = 0xF0;
/// Byte 1: hot-swap restore, `[MAGIC, KIND_RESTORE, key 64 B]`.
pub const KIND_RESTORE: u8 = 0xF1;
/// Byte 1: capability revocation, `[MAGIC, KIND_CAP_REVOKED, mask u32 LE]`.
pub const KIND_CAP_REVOKED: u8 = 0xF2;
/// Byte 1: shutdown, `[MAGIC, KIND_SHUTDOWN, reason]`.
pub const KIND_SHUTDOWN: u8 = 0xFF;

pub const SNAPSHOT_LEN: usize = HEADER_LEN + 8;
pub const RESTORE_KEY_LEN: usize = 64;
pub const RESTORE_LEN: usize = HEADER_LEN + RESTORE_KEY_LEN;
pub const CAP_REVOKED_LEN: usize = HEADER_LEN + 4;

/// Shutdown reason byte; any other value reads as "requested".
pub const SHUTDOWN_WATCHDOG: u8 = 1;
pub const SHUTDOWN_PARENT_DIED: u8 = 2;
