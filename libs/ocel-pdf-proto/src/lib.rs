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
/// Renewable idle lifetime; expired handles never become valid for a new owner.
pub const DOCUMENT_IDLE_TICKS: u64 = 6000;

/// Shared lifecycle policy independent of the native renderer. No caller may
/// renew, close or reclaim another nonexpired document.
pub struct DocumentOwner { caller: usize, handle: u32, activity: u64 }
impl DocumentOwner {
    pub fn new(caller: usize, handle: u32, now: u64) -> Self { Self { caller, handle, activity: now } }
    pub fn matches(&self, caller: usize, handle: u32) -> bool { self.caller == caller && self.handle == handle }
    pub fn expired(&self, now: u64) -> bool { now.wrapping_sub(self.activity) >= DOCUMENT_IDLE_TICKS }
    pub fn renew(&mut self, caller: usize, handle: u32, now: u64) -> bool {
        if !self.matches(caller, handle) || self.expired(now) { return false; }
        self.activity = now;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn foreign_caller_and_stale_handle_cannot_renew_document() {
        let mut owner = DocumentOwner::new(7, 1, 0);
        assert!(!owner.renew(8, 1, 50));
        assert!(!owner.renew(7, 2, 50));
        assert!(owner.renew(7, 1, 50));
        assert!(!owner.expired(50 + DOCUMENT_IDLE_TICKS - 1));
        assert!(owner.expired(50 + DOCUMENT_IDLE_TICKS));
        assert!(!owner.renew(7, 1, 50 + DOCUMENT_IDLE_TICKS));
    }
    #[test]
    fn replacement_owner_never_accepts_previous_handle() {
        let owner = DocumentOwner::new(8, 2, 100);
        assert!(!owner.matches(7, 1));
        assert!(!owner.matches(8, 1));
        assert!(owner.matches(8, 2));
    }
}

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
