use core::mem;

/// A marker trait for "Plain Old Data" types that are safe to interpret from raw bytes.
///
/// # Safety
///
/// Types implementing this trait must:
/// - Be `#[repr(C)]` or `#[repr(transparent)]`
/// - Have no padding bytes (or padding must be initialized/safe to read garbage)
/// - Be valid for any bit pattern of their underlying bytes
pub unsafe trait Pod: Sized {}

unsafe impl Pod for u8 {}
unsafe impl Pod for u16 {}
unsafe impl Pod for u32 {}
unsafe impl Pod for u64 {}
unsafe impl Pod for i8 {}
unsafe impl Pod for i16 {}
unsafe impl Pod for i32 {}
unsafe impl Pod for i64 {}
unsafe impl<T: Pod, const N: usize> Pod for [T; N] {}

// Helper to get bytes from a Pod type
pub fn as_bytes<T: Pod>(t: &T) -> &[u8] {
    unsafe {
        core::slice::from_raw_parts(
            t as *const T as *const u8,
            mem::size_of::<T>(),
        )
    }
}

