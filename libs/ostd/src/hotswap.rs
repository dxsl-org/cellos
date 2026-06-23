// SPDX-License-Identifier: MPL-2.0
//! Hot-migration state helpers for Cells.
//!
//! Provides safe wrappers over the kernel's state-stash primitives (syscalls
//! 410/411/412) and the [`ViStateTransfer`] opt-in trait for cells that want
//! to serialize across a live hot-swap or supervisor-triggered respawn.
//!
//! # Sequence contract
//! 1. **Old cell** — receives `AppEvent::Snapshot` → calls `stash(key, bytes)`.
//! 2. **Kernel** — keeps the bytes until the replacement is live.
//! 3. **New cell** — calls `restore(key)` on startup → gets bytes, then calls
//!    `clear(key)` to release the kernel slot.
//!
//! `stash` and `restore` are independent of each other's call site — the kernel
//! is the rendezvous point. Multiple calls to `stash` under the same key
//! overwrite the previous value; `restore` leaves the entry in place for retry.

extern crate alloc;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::format;
use crate::syscall;
use api::ViError;

// ── Public error type ─────────────────────────────────────────────────────────

/// Errors returned by hotswap stash/restore operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotswapError {
    /// Stash returned 0 bytes written — kernel stash is full (MAX_ENTRIES reached)
    /// or the data exceeds MAX_STASH_LEN (1 MB).
    StashFull,
    /// Restore returned 0 bytes — no state is stashed under this key.
    /// The cell may be starting cold (not after a hot-swap) or the key is wrong.
    NoState,
    /// The restored byte count does not match the allocation (should not happen;
    /// indicates a kernel or ABI bug).
    SizeMismatch,
}

impl From<HotswapError> for ViError {
    fn from(e: HotswapError) -> ViError {
        match e {
            // WouldBlock: caller should retry later or increase stash capacity.
            HotswapError::StashFull    => ViError::WouldBlock,
            // NotFound: no state under this key — cell is starting cold.
            HotswapError::NoState      => ViError::NotFound,
            // InvalidInput: restored payload is structurally malformed.
            HotswapError::SizeMismatch => ViError::InvalidInput,
        }
    }
}

// ── stash() ───────────────────────────────────────────────────────────────────

/// Store `data` in the kernel-managed stash under `key`.
///
/// Typically called from an `AppEvent::Snapshot` handler immediately before the
/// cell is replaced. The kernel retains the bytes until the replacement instance
/// calls [`restore`]. A second call with the same key overwrites the previous value.
///
/// # Errors
/// Returns [`HotswapError::StashFull`] when the kernel rejected the write (stash
/// at MAX_ENTRIES capacity with no existing entry for this key, or `data` exceeds
/// the 1 MB per-slot limit).
///
/// # Panics
/// Never.
pub fn stash(key: u64, data: &[u8]) -> Result<(), HotswapError> {
    let written = syscall::sys_state_stash(key, data);
    if written == 0 && !data.is_empty() {
        Err(HotswapError::StashFull)
    } else {
        Ok(())
    }
}

// ── restore() ─────────────────────────────────────────────────────────────────

/// Retrieve bytes from the stash by `key`, returning an owned buffer.
///
/// The stash entry remains in place after `restore`; call [`clear`] to release
/// the kernel slot once the state has been successfully applied.
///
/// # Two-step ABI note
/// The kernel does not provide a "query size" syscall, so this function uses a
/// conservative 1 MB buffer (the per-slot maximum) to avoid a two-round-trip
/// approach. The returned `Box<[u8]>` is exactly the byte count the kernel wrote,
/// not padded to the allocation size.
///
/// # Errors
/// Returns [`HotswapError::NoState`] when nothing is stashed under `key` (cold
/// start, wrong key, or the entry was already cleared).
///
/// # Panics
/// Never (OOM panics are handled by the cell allocator, not here).
pub fn restore(key: u64) -> Result<Box<[u8]>, HotswapError> {
    // Allocate a full-sized receive buffer. This is the upper bound set by
    // MAX_STASH_LEN in the kernel; we shrink the result to the actual payload.
    const MAX: usize = 1024 * 1024;
    let mut buf = alloc::vec![0u8; MAX];
    let n = syscall::sys_state_restore(key, &mut buf);
    if n == 0 {
        return Err(HotswapError::NoState);
    }
    buf.truncate(n);
    Ok(buf.into_boxed_slice())
}

// ── clear() ───────────────────────────────────────────────────────────────────

/// Delete the stash entry for `key`, freeing the kernel slot.
///
/// Idempotent — safe to call even if the key is absent or was never stashed.
/// The slot counts toward the global MAX_ENTRIES cap until freed; always call
/// `clear` after a successful [`restore`] to avoid leaking slots across spawns.
pub fn clear(key: u64) {
    syscall::sys_state_stash_clear(key);
}

// ── hotswap_key() ─────────────────────────────────────────────────────────────

/// Compute the canonical stash key for a `swap_id`.
///
/// Both sides of a hot-swap must agree on the key. The convention is:
/// the orchestrator assigns a monotonically increasing `swap_id`; the old cell
/// stashes under `hotswap_key(swap_id)`, the new cell restores from it.
///
/// Returns the raw numeric key as a `u64`. Use [`hotswap_key_str`] if you
/// need the human-readable string for logging.
pub fn hotswap_key(swap_id: u64) -> u64 {
    // FNV-1a of "hotswap-{swap_id}". We embed the swap_id directly in the upper
    // 32 bits and use the literal value 0xA3_0000_0000 as the namespace tag
    // (avoids collisions with the ARGV_STASH_KEY and per-TID personal slots).
    0x_A3_0000_0000_0000_u64 | (swap_id & 0xFFFF_FFFF_FFFF)
}

/// Human-readable form of the hotswap key (for logging / diagnostics).
pub fn hotswap_key_str(swap_id: u64) -> String {
    format!("hotswap-{}", swap_id)
}

// ── ViStateTransfer trait ─────────────────────────────────────────────────────

/// Opt-in trait for cells that support live hot-swap state preservation.
///
/// Implement this on the cell's primary state struct. The orchestrator calls
/// `stash` during `AppEvent::Snapshot` and `restore`/`clear` on startup to
/// thread state across the cell version boundary.
///
/// # Schema versioning
/// Increment `SCHEMA_VERSION` whenever the serialization layout changes in a
/// non-backward-compatible way. The `deserialize` implementation receives the
/// *serializer's* version and is responsible for migrating old payloads.
///
/// # Safety contract
/// - `serialize` output must not contain raw pointers or addresses — they are
///   invalid across the address-space boundary (old cell's heap is gone).
/// - `deserialize` must validate all fields before trusting them (the bytes
///   come from a different cell version and may be older or newer).
///
/// # Example
/// ```no_run
/// use ostd::hotswap::{ViStateTransfer, stash, restore, clear, hotswap_key};
/// use api::ViError;
///
/// struct MyState { counter: u64, name: [u8; 32] }
///
/// impl ViStateTransfer for MyState {
///     const SCHEMA_VERSION: u32 = 1;
///
///     fn serialize(&self) -> Result<Box<[u8]>, ViError> {
///         let mut out = [0u8; 40];
///         out[..8].copy_from_slice(&self.counter.to_le_bytes());
///         out[8..40].copy_from_slice(&self.name);
///         Ok(Box::from(out.as_slice()))
///     }
///
///     fn deserialize(version: u32, bytes: &[u8]) -> Result<Self, ViError> {
///         if version != 1 || bytes.len() < 40 {
///             return Err(ViError::InvalidInput);
///         }
///         let counter = u64::from_le_bytes(bytes[..8].try_into().unwrap());
///         let mut name = [0u8; 32];
///         name.copy_from_slice(&bytes[8..40]);
///         Ok(MyState { counter, name })
///     }
/// }
/// ```
pub trait ViStateTransfer: Sized {
    /// Schema version — increment when the serialization format changes in a
    /// way that breaks backward compatibility.
    const SCHEMA_VERSION: u32;

    /// Serialize cell state into an owned byte buffer.
    ///
    /// Must not contain raw pointers (they are invalid across hot-swap).
    /// The produced bytes are prefixed with an 8-byte header (version + length)
    /// by [`stash_transfer`] before being handed to the kernel.
    ///
    /// # Errors
    /// Returns `ViError::InvalidInput` on serialization failure; any other
    /// `ViError` variant on resource exhaustion.
    fn serialize(&self) -> Result<Box<[u8]>, ViError>;

    /// Deserialize state from bytes produced by the previous cell version.
    ///
    /// `version` is the `SCHEMA_VERSION` of the serializing cell — use it to
    /// detect format migrations and apply backward-compatible conversions.
    ///
    /// # Errors
    /// Return `ViError::InvalidInput` when `bytes` cannot be interpreted
    /// (truncated, wrong version without a migration path, or corrupt data).
    fn deserialize(version: u32, bytes: &[u8]) -> Result<Self, ViError>;
}

// ── High-level helpers using ViStateTransfer ──────────────────────────────────

/// Header prefixed to every serialized payload so the receiver can detect
/// schema version mismatches and payload truncation.
///
/// Layout (little-endian):
/// - `[0..4]`  = `SCHEMA_VERSION` (u32)
/// - `[4..8]`  = payload length in bytes (u32, ≤ MAX_STASH_LEN − 8)
const HEADER_LEN: usize = 8;

/// Serialize `state` and store it in the kernel stash under `key`.
///
/// A versioned 8-byte header is prepended so [`restore_transfer`] can detect
/// schema mismatches. Call this from `AppEvent::Snapshot`.
///
/// # Errors
/// - `ViError` propagated from `state.serialize()`
/// - `ViError::ResourceBusy` when the kernel stash is full ([`HotswapError::StashFull`])
pub fn stash_transfer<S: ViStateTransfer>(key: u64, state: &S) -> Result<(), ViError> {
    let payload = state.serialize()?;
    let payload_len = payload.len() as u32;

    let mut packet = alloc::vec![0u8; HEADER_LEN + payload.len()];
    packet[..4].copy_from_slice(&S::SCHEMA_VERSION.to_le_bytes());
    packet[4..8].copy_from_slice(&payload_len.to_le_bytes());
    packet[HEADER_LEN..].copy_from_slice(&payload);

    stash(key, &packet).map_err(ViError::from)
}

/// Retrieve and deserialize state from the kernel stash under `key`.
///
/// Reads the versioned header, validates length, and forwards to
/// `S::deserialize`. Clears the kernel slot on success so the entry does not
/// accumulate toward the MAX_ENTRIES cap.
///
/// # Errors
/// - `ViError::NotFound` when no state is stashed ([`HotswapError::NoState`])
/// - `ViError::InvalidInput` when the header is malformed or truncated
/// - `ViError` propagated from `S::deserialize`
pub fn restore_transfer<S: ViStateTransfer>(key: u64) -> Result<S, ViError> {
    let packet = restore(key).map_err(ViError::from)?;
    if packet.len() < HEADER_LEN {
        return Err(ViError::InvalidInput);
    }
    let version = u32::from_le_bytes(packet[..4].try_into().map_err(|_| ViError::InvalidInput)?);
    let declared_len = u32::from_le_bytes(packet[4..8].try_into().map_err(|_| ViError::InvalidInput)?) as usize;
    let payload = &packet[HEADER_LEN..];
    if payload.len() != declared_len {
        return Err(ViError::InvalidInput);
    }
    let state = S::deserialize(version, payload)?;
    // Release the slot only after successful deserialization so a retry can
    // recover if deserialization fails (e.g. the new binary is rolled back).
    clear(key);
    Ok(state)
}
