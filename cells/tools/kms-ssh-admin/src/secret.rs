//! Fixed-capacity byte buffer for key material: allocated once, never reallocated, wiped
//! (volatile, whole allocation) on drop. Growing a `Vec` would leave an unwiped copy of
//! the key text behind, so the capacity is fixed at construction and `push_slice` refuses
//! to exceed it.

use alloc::vec::Vec;

pub struct Secret {
    bytes: Vec<u8>,
}

impl Secret {
    /// Empty buffer that holds at most `capacity` bytes.
    pub fn with_capacity(capacity: usize) -> Self {
        Self { bytes: Vec::with_capacity(capacity) }
    }

    /// `len` zero bytes (capacity `len`), for decoding into.
    pub fn zeroed(len: usize) -> Self {
        Self { bytes: alloc::vec![0u8; len] }
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.bytes
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.bytes
    }

    /// Append `data`; false (and nothing appended) when it would exceed the capacity.
    pub fn push_slice(&mut self, data: &[u8]) -> bool {
        if data.len() > self.bytes.capacity() - self.bytes.len() {
            return false;
        }
        self.bytes.extend_from_slice(data);
        true
    }

    /// Append one byte under the same capacity rule.
    pub fn push(&mut self, byte: u8) -> bool {
        self.push_slice(&[byte])
    }

    /// Remove and wipe the last byte.
    pub fn pop(&mut self) {
        if let Some(last) = self.bytes.last_mut() {
            ostd::session::zeroize(core::slice::from_mut(last));
            self.bytes.pop();
        }
    }

    /// Keep the first `len` bytes; the removed tail is wiped first.
    pub fn truncate(&mut self, len: usize) {
        if len < self.bytes.len() {
            ostd::session::zeroize(&mut self.bytes[len..]);
            self.bytes.truncate(len);
        }
    }

    /// Wipe the whole allocation and empty the buffer (capacity is kept).
    pub fn wipe(&mut self) {
        ostd::session::zeroize_vec(&mut self.bytes);
    }

    /// Allocation address and capacity, so a test can look at what a wipe left behind.
    #[cfg(test)]
    pub fn allocation(&self) -> (*const u8, usize) {
        (self.bytes.as_ptr(), self.bytes.capacity())
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        self.wipe();
    }
}
