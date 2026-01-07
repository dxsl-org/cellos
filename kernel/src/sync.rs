//! Synchronization primitives.

use core::sync::atomic::{AtomicBool, Ordering};
use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};
use crate::hal::Arch;

/// Simple spinlock.
pub struct Spinlock<T> {
    lock: AtomicBool,
    data: UnsafeCell<T>,
}

unsafe impl<T: Send> Sync for Spinlock<T> {}
unsafe impl<T: Send> Send for Spinlock<T> {}

impl<T> Spinlock<T> {
    pub const fn new(data: T) -> Self {
        Self {
            lock: AtomicBool::new(false),
            data: UnsafeCell::new(data),
        }
    }

    pub fn lock(&self) -> SpinlockGuard<T> {
        // Disable interrupts to prevent ISR from deadlocking on this lock
        // We use crate::hal::ARCH directly. 
        // Note: Generic code in sync.rs depending on crate::hal is acceptable in this kernel structure.
        let saved_int = crate::hal::ARCH.interrupts_enabled();
        crate::hal::ARCH.disable_interrupts();
        
        while self.lock.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
            // Spin
            core::hint::spin_loop();
        }
        SpinlockGuard { lock: self, saved_int }
    }

    /// Force unlock the spinlock.
    /// 
    /// # Safety
    /// This is unsafe because it bypasses the lock guard.
    /// Should only be used in context switching or panic handlers.
    pub unsafe fn force_unlock(&self) {
        self.lock.store(false, Ordering::Release);
    }
}

pub struct SpinlockGuard<'a, T> {
    lock: &'a Spinlock<T>,
    saved_int: bool,
}

impl<'a, T> Deref for SpinlockGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.lock.data.get() }
    }
}

impl<'a, T> DerefMut for SpinlockGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.lock.data.get() }
    }
}

impl<'a, T> Drop for SpinlockGuard<'a, T> {
    fn drop(&mut self) {
        self.lock.lock.store(false, Ordering::Release);
        // Restore interrupt state
        if self.saved_int {
             crate::hal::ARCH.enable_interrupts();
        }
    }
}
