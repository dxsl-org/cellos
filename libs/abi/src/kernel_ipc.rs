// SPDX-License-Identifier: Apache-2.0
//! IPC constants the kernel itself speaks. `wire::ipc` re-exports them at
//! their service-contract path.

/// IPC payload buffer size.  Must be large enough for the largest serialized
/// message in the system — currently a VFS Write with ~900-byte content
/// (~916 bytes encoded).  4 KiB gives comfortable headroom.
pub const IPC_BUF_SIZE: usize = 4096;

/// Raw reactor wakeup opcode emitted within the same cell: `[0x12]`.
pub const REACTOR_WAKE_OPCODE: u8 = 0x12;
