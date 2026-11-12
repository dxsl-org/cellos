// SPDX-License-Identifier: Apache-2.0
//! Supervisor wire, carried inside the App SDK envelope
//! ([`super::app_envelope`]): `[MAGIC, KIND_MESSAGE, op, …]`.
//!
//! - `OP_HOTSWAP`: `{ target_service: [u8; 64], new_elf: [u8; 128] }`, 193 B.
//! - `OP_SNAPSHOT`: opcode only, 1 B.
//! - `OP_STATUS` reply: `[OP_STATUS, phase, result]`, 3 B.
//! - `OP_POWER`: `[OP_POWER, action]`, 2 B; action is the `SystemPower` a0.
//!
//! `OP_POWER` has the same byte as the feature-gated hostile-backend
//! `KILL_REQUEST_OPCODE`; the envelope keeps them apart, since kill requests
//! are raw frames.

use super::app_envelope::{KIND_MESSAGE, MAGIC};

pub const OP_HOTSWAP: u8 = 0x01;
pub const OP_SNAPSHOT: u8 = 0x02;
pub const OP_STATUS: u8 = 0x03;
pub const OP_POWER: u8 = 0x04;

/// Status phases (`OP_STATUS` byte 1).
pub const PHASE_REFUSED: u8 = 0;
pub const PHASE_SNAPSHOT: u8 = 1;
/// A refused `OP_POWER`; success has no reply.
pub const PHASE_POWER: u8 = 2;
pub const PHASE_HOTSWAP_DONE: u8 = 6;
/// Hot-swap failed; the result byte is a [`HotswapError`].
pub const PHASE_HOTSWAP_ERROR: u8 = 0xFF;

/// Status results (`OP_STATUS` byte 2) outside [`PHASE_HOTSWAP_ERROR`].
pub const STATUS_OK: u8 = 0x00;
pub const STATUS_UNAVAILABLE: u8 = 0x01;
pub const STATUS_REJECTED_CALLER: u8 = 0xFD;
pub const STATUS_SERVICE_NOT_FOUND: u8 = 0xFE;
pub const STATUS_INVALID_REQUEST: u8 = 0xFF;

pub const SVC_NAME_LEN: usize = 64;
pub const ELF_PATH_LEN: usize = 128;
pub const HOTSWAP_REQUEST_LEN: usize = 1 + SVC_NAME_LEN + ELF_PATH_LEN;
pub const SNAPSHOT_REQUEST_LEN: usize = 1;
pub const POWER_REQUEST_LEN: usize = 2;
pub const STATUS_REPLY_LEN: usize = 3;

pub const POWER_ACTION_REBOOT: u8 = 1;
pub const POWER_ACTION_POWEROFF: u8 = 2;

/// The only power request the kernel sends: the ACPI power button.
pub const POWER_BUTTON_REQUEST: [u8; 4] = [MAGIC, KIND_MESSAGE, OP_POWER, POWER_ACTION_POWEROFF];
/// Sender id of kernel-originated requests (the power button).
pub const KERNEL_SENDER: usize = 0;

/// Init → supervisor raw notice that a cell owner exited:
/// `[OWNER_EXITED_NOTICE, owner_tid usize LE]`, sent only by init (tid 1).
pub const OWNER_EXITED_NOTICE: u8 = 0xE1;
/// Supervisor → compositor relay of [`OWNER_EXITED_NOTICE`], same layout.
pub const OWNER_EXITED_RELAY: u8 = 0xE2;
pub const OWNER_EXITED_LEN: usize = 1 + core::mem::size_of::<usize>();

/// Result byte of a `PHASE_HOTSWAP_ERROR` status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum HotswapError {
    /// Target service name is not recognized or not currently live.
    ServiceNotFound = 0,
    /// Freeze syscall rejected (permission denied or tid invalid).
    FreezeFailed = 1,
    /// Unable to send Snapshot IPC to old cell.
    SnapshotIpcFailed = 2,
    /// Old cell did not stash state within the timeout.
    SnapshotTimeout = 3,
    /// Spawn of new ELF failed (path missing, no SpawnCap, loader error).
    SpawnFailed = 4,
    /// Unable to send Restore IPC to new cell.
    RestoreIpcFailed = 5,
    /// New cell did not call sys_hotswap_ready() within the timeout.
    ReadyTimeout = 6,
    /// Service mapping changed before it could be paused for snapshotting.
    PauseFailed = 7,
    /// Replacement became ready but could not be published as the provider.
    RegisterFailed = 8,
}

impl HotswapError {
    pub const fn from_u8(code: u8) -> Option<Self> {
        Some(match code {
            0 => Self::ServiceNotFound,
            1 => Self::FreezeFailed,
            2 => Self::SnapshotIpcFailed,
            3 => Self::SnapshotTimeout,
            4 => Self::SpawnFailed,
            5 => Self::RestoreIpcFailed,
            6 => Self::ReadyTimeout,
            7 => Self::PauseFailed,
            8 => Self::RegisterFailed,
            _ => return None,
        })
    }
}
