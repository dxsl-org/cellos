use core::cell::UnsafeCell;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicBool, Ordering};

/// Write-once storage for immutable data published during single-core boot.
///
/// Unlike a spinlock, publication uses no exclusive load/store instructions,
/// so it remains usable before the MMU assigns Normal memory attributes.
pub(super) struct BootOnce<T> {
    initialized: AtomicBool,
    value: UnsafeCell<MaybeUninit<T>>,
}

// SAFETY: initialization is restricted to the boot CPU before SMP starts;
// publication is release/acquire and the value is immutable afterward.
unsafe impl<T: Sync> Sync for BootOnce<T> {}

impl<T> BootOnce<T> {
    pub(super) const fn new() -> Self {
        Self {
            initialized: AtomicBool::new(false),
            value: UnsafeCell::new(MaybeUninit::uninit()),
        }
    }

    /// Publish the value exactly once before secondary CPUs or interrupts run.
    ///
    /// # Safety
    /// The caller must guarantee exclusive boot-time access with no concurrent
    /// readers or initializers. A repeated sequential call is rejected.
    #[cfg(any(test, target_arch = "riscv64", target_arch = "aarch64"))]
    pub(super) unsafe fn initialize(&self, value: T) {
        assert!(
            !self.initialized.load(Ordering::Relaxed),
            "boot value initialized more than once"
        );
        // SAFETY: guaranteed by the caller; the release store publishes the
        // completed write to later acquire readers.
        unsafe { (*self.value.get()).write(value) };
        self.initialized.store(true, Ordering::Release);
    }

    pub(super) fn get(&self) -> Option<&T> {
        if !self.initialized.load(Ordering::Acquire) {
            return None;
        }
        // SAFETY: an acquire load observed the release publication, and the
        // value is never mutated after initialization.
        Some(unsafe { (&*self.value.get()).assume_init_ref() })
    }
}

#[cfg(test)]
mod tests {
    use super::BootOnce;

    #[test]
    fn publishes_one_immutable_value() {
        let value = BootOnce::new();
        assert!(value.get().is_none());

        // SAFETY: this local test is single-threaded and initializes once.
        unsafe { value.initialize(42_u64) };

        assert_eq!(value.get(), Some(&42));
        assert_eq!(value.get(), Some(&42));
    }

    #[test]
    #[should_panic(expected = "boot value initialized more than once")]
    fn rejects_reinitialization() {
        let value = BootOnce::new();
        // SAFETY: both calls are single-threaded; the second must panic before writing.
        unsafe { value.initialize(1_u64) };
        unsafe { value.initialize(2_u64) };
    }
}
