// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Hot-swap state transfer interface.
//!
//! This module defines the `StateTransfer` trait used for live updates
//! of Cells without losing runtime state (e.g., active connections).
//!
//! See: `docs/architecture/03-runtime-model.md` § State Transfer

use crate::*;

/// Trait for transferring state during hot-swap operations.
///
/// # Protocol
/// 1. Kernel pauses `OldCell`
/// 2. `size = OldCell.state_size()`
/// 3. Kernel allocates buffer
/// 4. `OldCell.serialize_state(buffer)`
/// 5. Kernel loads `NewCell`
/// 6. `NewCell.deserialize_state(buffer)`
/// 7. Kernel unlinks `OldCell` and links `NewCell`
///
/// # Safety
/// Implementations must ensure that:
/// - Serialized state is version-compatible
/// - Deserialization handles missing/extra fields gracefully
/// - Critical resources (file handles, network connections) are properly transferred
pub trait ViStateTransfer {
    /// Get the size of the serialized state in bytes.
    ///
    /// # Returns
    /// The number of bytes required to serialize the current state.
    fn state_size(&self) -> usize;

    /// Serialize the current runtime state into a buffer.
    ///
    /// # Arguments
    /// * `buffer` - Destination buffer (must be at least `state_size()` bytes)
    ///
    /// # Returns
    /// Number of bytes written, or an error if serialization fails.
    /// Serialize the current runtime state into a buffer.
    ///
    /// # Arguments
    /// * `buffer` - Destination buffer (must be at least `state_size()` bytes)
    ///
    /// # Returns
    /// Number of bytes written, or an error if serialization fails.
    fn serialize_state(&self, buffer: &mut [u8]) -> ViResult<usize>;

    /// Deserialize and restore state from a buffer.
    ///
    /// # Arguments
    /// * `buffer` - Serialized state from a previous version
    ///
    /// # Returns
    /// `Ok(())` if state was successfully restored, or an error otherwise.
    fn deserialize_state(&mut self, buffer: &[u8]) -> ViResult<()>;
}
