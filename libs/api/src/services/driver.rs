// SPDX-License-Identifier: MPL-2.0

//! Driver trait interface.

use types::*;

/// Generic Driver Interface.
///
/// All ViCell drivers should implement this trait for uniform identification and control.
pub trait ViDriver: Send + Sync {
    /// Get the driver name.
    fn name(&self) -> &str;

    /// Initialize the driver.
    fn init(&mut self) -> ViResult<()>;
}
