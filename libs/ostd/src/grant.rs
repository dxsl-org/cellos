//! Typed linear handles for kernel Grant regions.
//!
//! A [`GrantHandle<T>`] wraps a raw grant ID (physical base address in SAS).
//! It is `!Copy + !Clone + !Send`: Rust affine ownership permits one wrapper,
//! while the kernel permits only the allocating **task** to free the grant.
//! Dropping the handle on that task calls `sys_grant_free`.
//!
//! `GrantShare` grants another task bounded access; it never transfers owner
//! authority. Cross-task users carry the raw ID only for access syscalls and
//! leave deallocation to the allocating task.
//!
//! # Typical flow
//!
//! ```rust,no_run
//! use ostd::grant::GrantHandle;
//!
//! // Allocate and use a 4-KiB region on one task.
//! let mut handle = GrantHandle::<u8>::alloc(4096).expect("OOM");
//! // ... fill/share/use the grant, while retaining this owner handle ...
//! drop(handle); // owner task frees it
//! ```

use core::marker::PhantomData;

use crate::syscall::{sys_grant_alloc, sys_grant_free, sys_grant_slice};

/// A typed, linear handle to a kernel-managed grant region.
///
/// `T` is a logical element type — the kernel manages raw byte pages and has
/// no knowledge of `T`. The region holds `len / size_of::<T>()` elements.
///
/// # Linear task-local invariant
/// `!Copy + !Clone + !Send`. At most one `GrantHandle<T>` exists per `grant_id`,
/// and it remains on the task that allocated the grant. Rust move semantics
/// prevent duplicate wrappers; kernel owner-TID checks enforce deallocation.
///
/// # Drop
/// `Drop` calls [`sys_grant_free`] from the allocating task, releasing the
/// frames back to the kernel. [`into_raw`](GrantHandle::into_raw) only detaches
/// that task-local Rust wrapper; it does not transfer kernel ownership.
pub struct GrantHandle<T> {
    id: usize,
    len: usize,
    _type: PhantomData<*mut T>,
}

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
        Some(Self {
            id,
            len: byte_len,
            _type: PhantomData,
        })
    }

    /// Reconstruct a raw grant ID previously detached on this same task.
    ///
    /// # Safety
    /// `id` must name a live grant allocated by the current task, and no other
    /// owner wrapper may exist. `len` must equal the byte length associated with
    /// the original `sys_grant_alloc`. `GrantShare`, IPC receipt, CellId
    /// equality, or access to the SAS address does not establish owner authority.
    /// Calling this twice with the same `id` creates duplicate owner wrappers.
    pub unsafe fn from_raw(id: usize, len: usize) -> Self {
        Self {
            id,
            len,
            _type: PhantomData,
        }
    }

    /// Consume the handle, returning `(grant_id, byte_len)` **without** freeing.
    ///
    /// This supports a same-task wrapper handoff to code that later reconstructs
    /// it with [`GrantHandle::from_raw`]. It does not change the kernel owner TID.
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
    pub fn id(&self) -> usize {
        self.id
    }

    /// Byte length of the grant region.
    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    /// True when the grant region has zero length (never the case for a
    /// successful `sys_grant_alloc`, but clippy requires `is_empty` beside `len`).
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

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

impl GrantHandle<u8> {
    /// Allocate a byte grant and initialize it from `data` before the handle escapes.
    ///
    /// `data` is copied into a freshly allocated grant owned exclusively by the
    /// returned handle. The copy happens before the handle is exposed to callers,
    /// preserving the linear-ownership invariant: no grant-sharing alias can
    /// observe partially initialized bytes.
    ///
    /// Returns `Some(handle)` when the kernel allocates a grant large enough to
    /// hold `data.len()` bytes; returns `None` on allocation failure or if the
    /// kernel refuses to map the new grant for initialization.
    pub fn alloc_copy_from_slice(data: &[u8]) -> Option<Self> {
        let handle = Self::alloc(data.len())?;
        let ptr = sys_grant_slice(handle.id)?;
        // SAFETY: `handle` uniquely owns a fresh grant that has not escaped this
        // function. `sys_grant_slice` returns the writable mapping for exactly
        // `handle.len` bytes, and `data.len() <= handle.len` by construction.
        unsafe {
            core::ptr::copy_nonoverlapping(data.as_ptr(), ptr, data.len());
        }
        Some(handle)
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
