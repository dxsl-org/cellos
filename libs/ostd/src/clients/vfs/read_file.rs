// SPDX-License-Identifier: MPL-2.0

extern crate alloc;

mod path;
mod session;
pub(super) mod wire;

use crate::service::VfsRef;
use crate::ViResult;
use alloc::vec::Vec;
use path::FileReadPlan;
use session::ReadSession;

pub(super) fn read_file(vfs: &mut VfsRef, path: &str, max_bytes: usize) -> ViResult<Vec<u8>> {
    let plan = FileReadPlan::parse(path)?;
    let mut session = ReadSession::new(vfs);
    let result = session.read(&plan, max_bytes);
    let cleanup = session.cleanup();
    match (result, cleanup) {
        (Ok(bytes), Ok(())) => Ok(bytes),
        (Err(err), _) => Err(err),
        (Ok(_), Err(err)) => Err(err),
    }
}

#[cfg(test)]
mod tests;
