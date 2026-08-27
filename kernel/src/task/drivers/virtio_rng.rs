/// VirtIO RNG stub — full MMIO probe deferred until a safe slot-skip strategy
/// is in place (probing already-claimed block/net slots hangs on RISC-V).
///
/// Production returns zero bytes when no hardware RNG is available. The
/// deterministic source below exists only to exercise the kernel's output-copy
/// transaction in an isolated `test-hooks` image.
pub fn init_driver() {}

#[cfg(feature = "test-hooks")]
static TEST_ENTROPY_ENABLED: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

#[cfg(feature = "test-hooks")]
static TEST_ENTROPY_REQUESTS: core::sync::atomic::AtomicUsize =
    core::sync::atomic::AtomicUsize::new(0);

#[cfg(feature = "test-hooks")]
pub(crate) struct TestEntropyGuard;

#[cfg(feature = "test-hooks")]
impl Drop for TestEntropyGuard {
    fn drop(&mut self) {
        TEST_ENTROPY_ENABLED.store(false, core::sync::atomic::Ordering::Release);
    }
}

#[cfg(feature = "test-hooks")]
pub(crate) fn enable_test_entropy() -> TestEntropyGuard {
    TEST_ENTROPY_ENABLED.store(true, core::sync::atomic::Ordering::Release);
    TEST_ENTROPY_REQUESTS.store(0, core::sync::atomic::Ordering::Release);
    TestEntropyGuard
}

/// Return test-only entropy requests since the latest guard activation.
#[cfg(feature = "test-hooks")]
pub(crate) fn test_entropy_requests() -> usize {
    TEST_ENTROPY_REQUESTS.load(core::sync::atomic::Ordering::Acquire)
}

pub fn get_random(buf: &mut [u8]) -> usize {
    #[cfg(feature = "test-hooks")]
    TEST_ENTROPY_REQUESTS.fetch_add(1, core::sync::atomic::Ordering::AcqRel);
    #[cfg(feature = "test-hooks")]
    if TEST_ENTROPY_ENABLED.load(core::sync::atomic::Ordering::Acquire) {
        for (index, byte) in buf.iter_mut().enumerate() {
            *byte = 0xA5 ^ index as u8;
        }
        return buf.len();
    }
    let _ = buf;
    0
}
