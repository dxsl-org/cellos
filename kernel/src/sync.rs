//! Synchronization primitives.

use crate::hal::Arch;
use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicBool, Ordering};

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

    pub fn lock(&self) -> SpinlockGuard<'_, T> {
        // Disable interrupts to prevent ISR from deadlocking on this lock
        // We use crate::hal::ARCH directly.
        // Note: Generic code in sync.rs depending on crate::hal is acceptable in this kernel structure.
        let saved_int = crate::hal::ARCH.interrupts_enabled();
        crate::hal::ARCH.disable_interrupts();

        while self
            .lock
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            // Spin
            core::hint::spin_loop();
        }
        SpinlockGuard {
            lock: self,
            saved_int,
        }
    }

    /// Acquire the lock only if it is free right now; never spins.
    ///
    /// # Returns
    /// `None` when the lock is already held, leaving the caller's interrupt
    /// state untouched.
    ///
    /// Intended for fault and panic teardown paths, where [`lock`](Self::lock)
    /// is unusable: the faulting context may itself be the holder, so spinning
    /// deadlocks. A `None` result is also information rather than mere failure —
    /// it proves the guarded data was mid-mutation when the fault landed, so
    /// reading it would observe a half-updated structure.
    pub fn try_lock(&self) -> Option<SpinlockGuard<'_, T>> {
        let saved_int = crate::hal::ARCH.interrupts_enabled();
        crate::hal::ARCH.disable_interrupts();

        if self
            .lock
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            Some(SpinlockGuard {
                lock: self,
                saved_int,
            })
        } else {
            // No guard is produced, so nothing will run the Drop that normally
            // restores this — do it here or the caller silently loses interrupts.
            if saved_int {
                crate::hal::ARCH.enable_interrupts();
            }
            None
        }
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
