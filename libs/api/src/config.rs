// SPDX-License-Identifier: MPL-2.0

//! Configuration API traits.

use crate::*;
use types::ViResult;

/// Configuration Service Interface.
pub trait ViConfig: Send + Sync {
    /// Get a configuration value (Zero-Copy).
    /// Returns a reference to the string in the Service's memory.
    fn get(&self, key: &str) -> ViResult<&str>;

    /// Set a configuration value.
    fn set(&mut self, key: &str, value: &str) -> ViResult<()>;
}
