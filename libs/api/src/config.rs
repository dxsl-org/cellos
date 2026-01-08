// SPDX-License-Identifier: MPL-2.0

//! Configuration API traits.

use crate::*;
use types::ViResult;

/// Configuration Service Interface.
pub trait ViConfig: Send + Sync {
    /// Get a configuration value (Zero-Copy).
    /// Returns (Pointer, Length) to the value in the Service's memory.
    fn get(&self, key: &str) -> ViResult<(usize, usize)>;

    /// Set a configuration value.
    fn set(&self, key: &str, value: &str) -> ViResult<()>;

    /// Subscribe to changes for a key.
    fn subscribe(&self, key: &str, subscriber_cell_id: usize) -> ViResult<()>;
}
