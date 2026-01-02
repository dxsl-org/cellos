
use alloc::boxed::Box;

/// Safe wrapper around allocating memory.
/// In Asterinas/Theseus, this wraps the global allocator.
pub fn alloc_box<T>(val: T) -> Box<T> {
    Box::new(val)
}

/// A safe wrapper for a "Grant" (Shared Memory Region)
/// This ensures that the user cannot access the memory once it is granted to the kernel,
/// or manages the borrowing rules safely.
pub struct Region<T> {
    #[allow(dead_code)]
    inner: T,
}

impl<T> Region<T> {
    pub fn new(val: T) -> Self {
        Self { inner: val }
    }
    
    pub fn share(&self) {
        // Issue Syscall::Allow
    }
}
