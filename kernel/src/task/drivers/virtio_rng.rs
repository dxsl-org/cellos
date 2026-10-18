//! Kernel entropy source: x86_64 uses CPUID-gated RDSEED/RDRAND.
//!
//! VirtIO MMIO probing remains disabled on other architectures until a safe
//! slot-skip strategy exists (probing claimed block/net slots hangs on RISC-V).
//! Those targets return zero bytes unless the isolated RV64 output-copy fixture
//! explicitly enables its deterministic test source.
pub fn init_driver() {
    #[cfg(target_arch = "x86_64")]
    let _ = x86_rng::features();
}

#[cfg(all(feature = "getrandom-sas-test", target_arch = "riscv64"))]
static TEST_ENTROPY_ENABLED: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

#[cfg(all(feature = "getrandom-sas-test", target_arch = "riscv64"))]
static TEST_ENTROPY_REQUESTS: core::sync::atomic::AtomicUsize =
    core::sync::atomic::AtomicUsize::new(0);

#[cfg(all(feature = "getrandom-sas-test", target_arch = "riscv64"))]
pub(crate) struct TestEntropyGuard;

#[cfg(all(feature = "getrandom-sas-test", target_arch = "riscv64"))]
impl Drop for TestEntropyGuard {
    fn drop(&mut self) {
        TEST_ENTROPY_ENABLED.store(false, core::sync::atomic::Ordering::Release);
    }
}

#[cfg(all(feature = "getrandom-sas-test", target_arch = "riscv64"))]
pub(crate) fn enable_test_entropy() -> TestEntropyGuard {
    TEST_ENTROPY_ENABLED.store(true, core::sync::atomic::Ordering::Release);
    TEST_ENTROPY_REQUESTS.store(0, core::sync::atomic::Ordering::Release);
    TestEntropyGuard
}

/// Return test-only entropy requests since the latest guard activation.
#[cfg(all(feature = "getrandom-sas-test", target_arch = "riscv64"))]
pub(crate) fn test_entropy_requests() -> usize {
    TEST_ENTROPY_REQUESTS.load(core::sync::atomic::Ordering::Acquire)
}

/// Fill a prefix with hardware entropy, returning its length.
///
/// Zero means no secure source is available or its bounded retries exhausted.
/// A short read contains only successful hardware samples, never filler bytes.
pub fn get_random(buf: &mut [u8]) -> usize {
    #[cfg(all(feature = "getrandom-sas-test", target_arch = "riscv64"))]
    TEST_ENTROPY_REQUESTS.fetch_add(1, core::sync::atomic::Ordering::AcqRel);
    #[cfg(all(feature = "getrandom-sas-test", target_arch = "riscv64"))]
    if TEST_ENTROPY_ENABLED.load(core::sync::atomic::Ordering::Acquire) {
        for (index, byte) in buf.iter_mut().enumerate() {
            *byte = 0xA5 ^ index as u8;
        }
        return buf.len();
    }
    #[cfg(target_arch = "x86_64")]
    {
        x86_rng::get_random(buf)
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        let _ = buf;
        0
    }
}

#[cfg(target_arch = "x86_64")]
mod x86_rng {
    use core::arch::x86_64::{__cpuid, __cpuid_count, _rdrand64_step, _rdseed64_step};
    use core::sync::atomic::{AtomicU8, Ordering};

    const DETECTED: u8 = 1;
    const RDSEED: u8 = 2;
    const RDRAND: u8 = 4;
    const ATTEMPTS: usize = 10;
    static FEATURES: AtomicU8 = AtomicU8::new(0);

    pub(super) fn features() -> u8 {
        let cached = FEATURES.load(Ordering::Relaxed);
        if cached != 0 {
            return cached;
        }
        let max_leaf = __cpuid(0).eax;
        let mut detected = DETECTED;
        if max_leaf >= 1 && __cpuid(1).ecx & (1 << 30) != 0 {
            detected |= RDRAND;
        }
        if max_leaf >= 7 && __cpuid_count(7, 0).ebx & (1 << 18) != 0 {
            detected |= RDSEED;
        }
        // The feature bits are the entire cached state; no dependent memory
        // needs publication. Concurrent initial probes are harmless.
        FEATURES.store(detected, Ordering::Relaxed);
        detected
    }

    fn retry(mut sample: impl FnMut() -> Option<u64>) -> Option<u64> {
        for _ in 0..ATTEMPTS {
            if let Some(word) = sample() {
                return Some(word);
            }
            core::hint::spin_loop();
        }
        None
    }

    fn next_word(features: u8) -> Option<u64> {
        // Prefer RDSEED's conditioned seed entropy. Exhaustion may fall back
        // only to the CPU's cryptographic RDRAND generator, never a software PRNG.
        if features & RDSEED != 0 {
            let word = retry(|| {
                let mut word = 0;
                // SAFETY: CPUID.7.0:EBX[18] gated entry to this instruction.
                let ready = unsafe { _rdseed64_step(&mut word) };
                (ready == 1).then_some(word)
            });
            if word.is_some() {
                return word;
            }
        }
        if features & RDRAND != 0 {
            return retry(|| {
                let mut word = 0;
                // SAFETY: CPUID.1:ECX[30] gated entry to this instruction.
                let ready = unsafe { _rdrand64_step(&mut word) };
                (ready == 1).then_some(word)
            });
        }
        None
    }

    fn fill(buf: &mut [u8], mut sample: impl FnMut() -> Option<u64>) -> usize {
        let mut written = 0;
        for chunk in buf.chunks_mut(core::mem::size_of::<u64>()) {
            let Some(word) = sample() else {
                break;
            };
            chunk.copy_from_slice(&word.to_le_bytes()[..chunk.len()]);
            written += chunk.len();
        }
        written
    }

    pub(super) fn get_random(buf: &mut [u8]) -> usize {
        let features = features();
        fill(buf, || next_word(features))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn retry_exhaustion_is_bounded_and_zero_is_a_valid_sample() {
            let mut calls = 0;
            assert_eq!(retry(|| {
                calls += 1;
                None
            }), None);
            assert_eq!(calls, ATTEMPTS);

            calls = 0;
            assert_eq!(retry(|| {
                calls += 1;
                (calls == ATTEMPTS).then_some(0)
            }), Some(0));
            assert_eq!(calls, ATTEMPTS);
        }

        #[test]
        fn exhausted_source_returns_only_the_successful_prefix() {
            let mut buf = [0xA5; 17];
            let mut calls = 0;
            let written = fill(&mut buf, || {
                calls += 1;
                (calls == 1).then_some(0x0807_0605_0403_0201)
            });
            assert_eq!(written, 8);
            assert_eq!(calls, 2);
            assert_eq!(&buf[..8], &[1, 2, 3, 4, 5, 6, 7, 8]);
            assert_eq!(&buf[8..], &[0xA5; 9]);
        }

        #[test]
        fn final_partial_word_is_bounded_and_empty_output_consumes_nothing() {
            let mut buf = [0xA5; 9];
            let mut calls = 0;
            assert_eq!(fill(&mut buf, || {
                calls += 1;
                Some(0)
            }), 9);
            assert_eq!(calls, 2);
            assert_eq!(buf, [0; 9]);
            assert_eq!(fill(&mut [], || panic!("empty output sampled entropy")), 0);
        }
    }
}
