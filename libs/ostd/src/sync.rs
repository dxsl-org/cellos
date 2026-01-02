use crate::syscall::{sys_futex_wait, sys_futex_wake, SyscallResult, SyscallError};
use core::sync::atomic::{AtomicU32, Ordering};
use core::cell::UnsafeCell;

/// A mutual exclusion primitive useful for protecting shared data
pub struct Mutex<T: ?Sized> {
    lock: AtomicU32, // 0: unlocked, 1: locked, 2: locked + waiter
    data: UnsafeCell<T>,
}

unsafe impl<T: ?Sized + Send> Sync for Mutex<T> {}
unsafe impl<T: ?Sized + Send> Send for Mutex<T> {}

impl<T> Mutex<T> {
    pub const fn new(data: T) -> Self {
        Self {
            lock: AtomicU32::new(0),
            data: UnsafeCell::new(data),
        }
    }
}

impl<T: ?Sized> Mutex<T> {
    pub fn lock(&self) -> MutexGuard<'_, T> {
        // Fast path: 0 -> 1
        if self.lock.compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed).is_ok() {
            return MutexGuard { lock: self };
        }

        // Slow path
        loop {
            // Check current reservation
            let mut state = self.lock.load(Ordering::Relaxed);
            if state == 2 {
                 // Already has waiters, just sleep
                 let _ = sys_futex_wait(&self.lock, 2);
            } else {
                 // Try to acquire or upgrade to 2
                 // If 0 -> 2: Acquired!
                 // If 1 -> 2: Marked as contended
                 if self.lock.compare_exchange(state, 2, Ordering::Acquire, Ordering::Relaxed).is_ok() {
                     if state == 0 {
                         // We got it
                         return MutexGuard { lock: self };
                     }
                     // Otherwise we just marked it as 2, now we sleep
                 }
            }
        }
    }
}

pub struct MutexGuard<'a, T: ?Sized> {
    lock: &'a Mutex<T>,
}

impl<'a, T: ?Sized> core::ops::Deref for MutexGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.lock.data.get() }
    }
}

impl<'a, T: ?Sized> core::ops::DerefMut for MutexGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.lock.data.get() }
    }
}

impl<'a, T: ?Sized> Drop for MutexGuard<'a, T> {
    fn drop(&mut self) {
        // Unlock: if we were 1, just set 0. If 2, set 0 and wake.
        // We use swap because we are the owner.
        if self.lock.lock.swap(0, Ordering::Release) == 2 {
            // Wake 1 waiter
            let _ = sys_futex_wake(&self.lock.lock, 1);
        }
    }
}

/// A Condition Variable
pub struct Condvar {
    futex: AtomicU32,
}

impl Condvar {
    pub const fn new() -> Self {
        Self {
            futex: AtomicU32::new(0),
        }
    }

    pub fn wait<'a, T>(&self, guard: MutexGuard<'a, T>) -> MutexGuard<'a, T> {
        let mutex = guard.lock;
        let value = self.futex.load(Ordering::Relaxed);
        
        // Unlock mutex
        drop(guard);
        
        // Wait on futex
        let _ = sys_futex_wait(&self.futex, value);
        
        // Re-lock mutex
        mutex.lock()
    }

    pub fn notify_one(&self) {
        self.futex.fetch_add(1, Ordering::Relaxed);
        let _ = sys_futex_wake(&self.futex, 1);
    }

    pub fn notify_all(&self) {
        self.futex.fetch_add(1, Ordering::Relaxed);
        let _ = sys_futex_wake(&self.futex, usize::MAX);
    }
}
