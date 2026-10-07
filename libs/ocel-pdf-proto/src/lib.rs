// SPDX-License-Identifier: MPL-2.0
//! Bounded, copied IPC for Ocel's isolated native PDF renderer.
#![no_std]
#![forbid(unsafe_code)]
extern crate alloc;
use alloc::{string::String, vec::Vec};
use serde::{Deserialize, Serialize};

pub const IPC_BYTES: usize = 4096;
pub const PIXEL_CHUNK_BYTES: usize = 3072;
pub const MAX_DOCUMENT_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_PAGE_PIXELS: usize = 1024 * 1024;

/// Document handles are scoped to the sender TID. Closing a document also drops
/// its cached raster. Pixels are straight, opaque BGRA8888, row-major.
#[derive(Debug, Serialize, Deserialize)]
pub enum Request {
    Open { path: String },
    RenderPage { document: u32, page: u32, max_width: u32, max_height: u32 },
    ReadPixels { document: u32, offset: u32, length: u16 },
    Close { document: u32 },
    /// Readiness probe; does not open a document or allocate a raster.
    Ping,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Response {
    Opened { document: u32, pages: u32 },
    Rendered { width: u32, height: u32, bytes: u32 },
    Pixels { offset: u32, bytes: Vec<u8> },
    Closed,
    Error { message: String },
    Ready,
}
