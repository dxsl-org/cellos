//! Synchronization primitives.

#[cfg(not(all(test, not(target_os = "none"))))]
use crate::hal::Arch;
use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

pub mod rank;
pub use rank::Rank;

/// Spin hook: answers a peer's pending TLB-flush request while this hart spins
/// with interrupts masked. Set once by SMP bring-up ([`install_hart_hooks`]);
/// until then there are no peers and nothing is owed.
static SPIN_HOOK: AtomicUsize = AtomicUsize::new(0);
/// This hart's id if its hart-local state is installed (`None` before), for
/// the spin hook and the per-hart held-rank mask. Hart 0 until the hook is set.
static HART_ID_HOOK: AtomicUsize = AtomicUsize::new(0);

/// Installs the hart-id reader and the spin hook. Called once on the boot hart
/// before any secondary starts; the hooks are plain functions that take no lock,
/// allocate nothing and never log.
pub fn install_hart_hooks(hart_id: fn() -> Option<usize>, spin: fn(usize)) {
    HART_ID_HOOK.store(hart_id as usize, Ordering::Release);
    SPIN_HOOK.store(spin as usize, Ordering::Release);
}

// Host tests neither spin-answer nor keep per-hart masks (they use a thread-local).
#[cfg_attr(all(test, not(target_os = "none")), allow(dead_code))]
fn installed_hart_id() -> Option<usize> {
    // Relaxed: the hook is published once, before any secondary starts; a hart
    // that still reads 0 is the single boot hart.
    match HART_ID_HOOK.load(Ordering::Relaxed) {
        0 => Some(0),
        // SAFETY: only `install_hart_hooks` stores here, always a
        // `fn() -> Option<usize>`.
        hook => unsafe { core::mem::transmute::<usize, fn() -> Option<usize>>(hook)() },
    }
}

/// Simple spinlock.
pub struct Spinlock<T> {
    lock: AtomicBool,
    #[cfg(any(feature = "test-hooks", test))]
    rank: Rank,
    data: UnsafeCell<T>,
}

unsafe impl<T: Send> Sync for Spinlock<T> {}
unsafe impl<T: Send> Send for Spinlock<T> {}

impl<T> Spinlock<T> {
    /// An unranked lock: instance locks inside per-object structs.
    pub const fn new(data: T) -> Self {
        Self::ranked(data, Rank::UNRANKED)
    }

    /// A lock with a place in the global order ([`rank`]); every `static`
    /// lock is built this way. The rank is checked only in the test posture.
    #[cfg_attr(not(any(feature = "test-hooks", test)), allow(unused_variables))]
    pub const fn ranked(data: T, rank: Rank) -> Self {
        Self {
            lock: AtomicBool::new(false),
            #[cfg(any(feature = "test-hooks", test))]
            rank,
            data: UnsafeCell::new(data),
        }
    }

    pub fn lock(&self) -> SpinlockGuard<'_, T> {
        // Disable interrupts to prevent ISR from deadlocking on this lock
        // We use crate::hal::ARCH directly.
        // Note: Generic code in sync.rs depending on crate::hal is acceptable in this kernel structure.
        #[cfg(not(all(test, not(target_os = "none"))))]
        let saved_int = crate::hal::ARCH.interrupts_enabled();
        #[cfg(not(all(test, not(target_os = "none"))))]
        crate::hal::ARCH.disable_interrupts();
        #[cfg(any(feature = "test-hooks", test))]
        rank::before_acquire(self.rank);

        loop {
            // Every acquisition, not only a contended one, answers an
            // invalidation a peer asked of this CPU. x86 syscalls run with IF
            // masked (SYSCALL FMASK), so the IPI cannot land until the syscall
            // returns: a long one that takes locks but never waits on them
            // (spawn reading a 627 KiB image out of VIFS1) starved the peer's
            // shootdown past its probe budget. A peer may also hold this very
            // lock while it waits — on RV64/AArch64 too, now that hart 0 is a
            // shootdown target like every other hart. Interrupts are masked
            // here, so service only the lock-free, log-free local mailbox:
            // never schedule, allocate, or log from this ownership window.
            // With nothing owed it is a hart-id read and two loads.
            #[cfg(all(cellos_wide, not(all(test, not(target_os = "none")))))]
            match SPIN_HOOK.load(Ordering::Relaxed) {
                0 => {}
                // SAFETY: only `install_hart_hooks` stores here, always a `fn(usize)`.
                // An uninstalled hart answers hart 0's mailbox, as
                // `current_hart_id` always did.
                hook => unsafe {
                    core::mem::transmute::<usize, fn(usize)>(hook)(installed_hart_id().unwrap_or(0))
                },
            }
            if self
                .lock
                .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
                .is_ok()
            {
                break;
            }
            core::hint::spin_loop();
        }
        #[cfg(any(feature = "test-hooks", test))]
        rank::acquired(self.rank);
        SpinlockGuard {
            lock: self,
            #[cfg(not(all(test, not(target_os = "none"))))]
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
        #[cfg(not(all(test, not(target_os = "none"))))]
        let saved_int = crate::hal::ARCH.interrupts_enabled();
        #[cfg(not(all(test, not(target_os = "none"))))]
        crate::hal::ARCH.disable_interrupts();

        if self
            .lock
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            #[cfg(any(feature = "test-hooks", test))]
            rank::acquired(self.rank);
            Some(SpinlockGuard {
                lock: self,
                #[cfg(not(all(test, not(target_os = "none"))))]
                saved_int,
            })
        } else {
            // No guard is produced, so nothing will run the Drop that normally
            // restores this — do it here or the caller silently loses interrupts.
            #[cfg(not(all(test, not(target_os = "none"))))]
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
        #[cfg(any(feature = "test-hooks", test))]
        rank::released(self.rank);
        self.lock.store(false, Ordering::Release);
    }
}

pub struct SpinlockGuard<'a, T> {
    lock: &'a Spinlock<T>,
    #[cfg(not(all(test, not(target_os = "none"))))]
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
        #[cfg(any(feature = "test-hooks", test))]
        rank::released(self.lock.rank);
        self.lock.lock.store(false, Ordering::Release);
        // Restore interrupt state
        #[cfg(not(all(test, not(target_os = "none"))))]
        if self.saved_int {
            crate::hal::ARCH.enable_interrupts();
        }
    }
}
