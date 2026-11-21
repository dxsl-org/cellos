// SPDX-License-Identifier: MPL-2.0
//! `icache`: publishing loaded code to instruction fetch.
//!
//! x86_64 keeps instruction fetch coherent with stores on every CPU, so loaded
//! code needs no maintenance and no remote request exists.

use crate::memory::paging::Flags;
use types::{VAddr, ViResult};

/// Nothing to serve: instruction fetch is coherent.
pub(crate) fn service_sync_request(_hart_id: usize) {}

/// Nothing to publish: instruction fetch is coherent with the loader's stores.
pub(crate) fn sync_loaded_code(_pages: &[(VAddr, Flags)], _cell: &str) -> ViResult<()> {
    Ok(())
}
