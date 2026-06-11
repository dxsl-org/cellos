//! Typed linear handles for kernel Grant regions.
//!
//! A [`GrantHandle<T>`] wraps a raw grant ID (physical base address in SAS).
//! It is `!Copy + !Clone` — Rust affine ownership enforces that each grant
//! has exactly one live owner at a time. Dropping the handle automatically
//! calls `sys_grant_free`, returning the frames to the kernel.
//!
//! This implements the Singularity "exchange heap" lesson: ownership transfer
//! of shared memory is explicit and compile-time verifiable, not a runtime
//! convention that can be accidentally violated.
//!
//! # Typical flow
//!
//! ```rust,no_run
//! // Allocate a 4-KiB region typed as raw bytes.
//! let mut handle = GrantHandle::<u8>::alloc(4096).expect("OOM");
//! // ... fill with data ...
//! let (id, len) = handle.into_raw();          // hand off to another Cell via IPC
//! // The grant is NOT freed here — receiver calls GrantHandle::from_raw(id, len).
//! ```

use core::marker::PhantomData;

use crate::syscall::{sys_grant_alloc, sys_grant_free, sys_grant_slice};

/// A typed, linear handle to a kernel-managed grant region.
///
/// `T` is a logical element type — the kernel manages raw byte pages and has
/// no knowledge of `T`. The region holds `len / size_of::<T>()` elements.
///
/// # Linear type invariant
/// `!Copy + !Clone`. At most one `GrantHandle<T>` exists per `grant_id` in
/// this cell's address space. This is enforced by Rust's affine type system
/// (move semantics), not by hardware.
///
/// # Drop
/// `Drop` calls [`sys_grant_free`], releasing the frames back to the kernel.
/// Use [`into_raw`](GrantHandle::into_raw) to transfer ownership without freeing.
pub struct GrantHandle<T> {
    id: usize,
    len: usize,
    _type: PhantomData<*mut T>,
}

// SAFETY: In SAS all cells share the address space. GrantHandle is Send because
// only one cell can own the handle at a time (move semantics prevent aliasing).
unsafe impl<T: Send> Send for GrantHandle<T> {}

impl<T> GrantHandle<T> {
    /// Allocate a new grant region holding `count` elements of type `T`.
    ///
    /// The kernel allocates page-aligned contiguous frames; the actual region
    /// may be slightly larger than `count * size_of::<T>()` due to alignment.
    ///
    /// Returns `None` on OOM or if `count * size_of::<T>()` overflows.
    pub fn alloc(count: usize) -> Option<Self> {
        let byte_len = count.checked_mul(core::mem::size_of::<T>())?;
        let byte_len = byte_len.max(1); // zero-size grants are not useful
        let id = sys_grant_alloc(byte_len)?;
        Some(Self { id, len: byte_len, _type: PhantomData })
    }

    /// Wrap a raw grant ID received from another Cell via IPC.
    ///
    /// # Safety
    /// `id` must be a valid grant owned by this cell (e.g. via `sys_grant_share`
    /// + the receiver's confirmation). `len` must equal the byte length returned
    /// by the original `sys_grant_alloc`. Calling this twice with the same `id`
    /// creates two owners and will double-free on drop — undefined behaviour.
    pub unsafe fn from_raw(id: usize, len: usize) -> Self {
        Self { id, len, _type: PhantomData }
    }

    /// Consume the handle, returning `(grant_id, byte_len)` **without** freeing.
    ///
    /// Use when passing grant ownership to another Cell via IPC. The receiver
    /// must call [`GrantHandle::from_raw`] to re-wrap it.
    #[inline]
    pub fn into_raw(self) -> (usize, usize) {
        let id = self.id;
        let len = self.len;
        // Prevent Drop from calling sys_grant_free — ownership is transferred.
        core::mem::forget(self);
        (id, len)
    }

    /// Raw kernel grant ID (physical base address in SAS == virtual address).
    #[inline]
    pub fn id(&self) -> usize { self.id }

    /// Byte length of the grant region.
    #[inline]
    pub fn len(&self) -> usize { self.len }

    /// Get an exclusive byte slice over the entire grant region.
    ///
    /// # Safety
    /// No other live reference to this grant must exist. The handle must still
    /// be valid (not freed). The caller must ensure the data at `[0, len)` is
    /// initialised before reading.
    pub unsafe fn as_bytes_mut(&mut self) -> &mut [u8] {
        let ptr = sys_grant_slice(self.id)
            .expect("GrantHandle::as_bytes_mut: grant not found or permission denied");
        core::slice::from_raw_parts_mut(ptr, self.len)
    }
}

impl<T: Copy> GrantHandle<T> {
    /// Get an exclusive typed slice over the grant region.
    ///
    /// # Safety
    /// No other live reference must exist. All bytes must be initialised as
    /// valid `T` values (bitwise validity).
    pub unsafe fn as_slice_mut(&mut self) -> &mut [T] {
        let ptr = sys_grant_slice(self.id)
            .expect("GrantHandle::as_slice_mut: grant not found or permission denied");
        let count = self.len / core::mem::size_of::<T>();
        core::slice::from_raw_parts_mut(ptr as *mut T, count)
    }
}

impl<T> Drop for GrantHandle<T> {
    fn drop(&mut self) {
        sys_grant_free(self.id);
    }
}
