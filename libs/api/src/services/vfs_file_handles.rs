// SPDX-License-Identifier: Apache-2.0
//! Service-issued VFS file handles.
//!
//! Unlike directory handles, these are never carried by the kernel across
//! spawn. They name only VFS-local table entries.

use serde::{Deserialize, Serialize};

/// An opaque file handle issued by the VFS service.
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ViVfsFileHandle(pub u64);
