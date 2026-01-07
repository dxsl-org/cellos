//! Filesystem subsystem.

use crate::prelude::*;
use crate::sync::Spinlock;

pub mod pathbuf;

/// Global root filesystem
pub static ROOT_FS: Spinlock<Option<Arc<dyn api::fs::FileSystem>>> = Spinlock::new(None);

/// File types (re-exported from API or defined here if kernel specific)
// pub use api::fs::{FileType, Inode, DirStream, DirectoryEntry, FileSystem};
pub use api::fs::FileSystem;

/// Block on a future (synchronous execution for kernel)
pub fn block_on<F: core::future::Future>(mut future: F) -> F::Output {
    use core::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
    
    // Dummy waker that does nothing
    unsafe fn clone(_: *const ()) -> RawWaker { RawWaker::new(core::ptr::null(), &VTABLE) }
    unsafe fn wake(_: *const ()) {}
    unsafe fn wake_by_ref(_: *const ()) {}
    unsafe fn drop(_: *const ()) {}
    
    static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, wake, wake_by_ref, drop);
    let waker = unsafe { Waker::from_raw(RawWaker::new(core::ptr::null(), &VTABLE)) };
    let mut context = Context::from_waker(&waker);
    
    // Simple polling loop
    loop {
        match unsafe { core::pin::Pin::new_unchecked(&mut future).poll(&mut context) } {
            Poll::Ready(val) => return val,
            Poll::Pending => {
                // In a real kernel, we would yield or wait for interrupts
                // For now, busy loop or hint to CPU
                core::hint::spin_loop();
            }
        }
    }
}
