/// BrokerRng — VirtIO-RNG-backed ChaCha20 PRNG for Noise ephemeral keygen.
///
/// Mirrors `cells/services/net/src/tls/rng.rs`. Panic policy: if the kernel
/// returns 0 bytes from GetRandom after 64 attempts the device is absent —
/// the broker must not start the crypto transport without real entropy (the
/// silent xorshift32 fallback in kernel/src/task/syscall.rs:2493 makes Noise
/// ephemerals predictable if we proceed).
use rand_chacha::ChaCha20Rng;
use rand_core::{CryptoRng, RngCore, SeedableRng};
use ostd::syscall::sys_get_random;

/// ChaCha20 PRNG seeded from VirtIO-RNG hardware entropy.
///
/// Implements `clatter::traits::Rng` (= `RngCore + CryptoRng + Default + Clone`).
/// `Default` calls `new_seeded()` so clatter's internal ephemeral keygen is
/// also fail-closed — if it ever calls `RNG::default()`, it panics correctly.
#[derive(Clone)]
pub struct BrokerRng(ChaCha20Rng);

impl BrokerRng {
    /// Seed from VirtIO-RNG. Loops until 32 bytes are filled.
    ///
    /// # Panics
    /// Panics if VirtIO-RNG is absent (returns 0 bytes after 64 attempts).
    pub fn new_seeded() -> Self {
        let mut seed = [0u8; 32];
        let mut filled = 0usize;
        let mut attempts = 0u32;
        while filled < 32 {
            let n = sys_get_random(&mut seed[filled..]);
            if n > 0 {
                filled += n;
            } else {
                attempts += 1;
                assert!(
                    attempts < 64,
                    "[net-broker] VirtIO-RNG absent — cannot generate Noise ephemerals. \
                     Add -object rng-random,id=rng0 -device virtio-rng-device,rng=rng0 to QEMU."
                );
                for _ in 0..1000 {
                    core::hint::spin_loop();
                }
            }
        }
        Self(ChaCha20Rng::from_seed(seed))
    }
}

/// Fail-closed Default: new_seeded() panics if VirtIO-RNG is absent.
/// Required by clatter's Rng bound; ensures ANY internal ephemeral keygen
/// in clatter is also fail-closed, not just explicit calls.
impl Default for BrokerRng {
    fn default() -> Self {
        Self::new_seeded()
    }
}

impl RngCore for BrokerRng {
    fn next_u32(&mut self) -> u32 { self.0.next_u32() }
    fn next_u64(&mut self) -> u64 { self.0.next_u64() }
    fn fill_bytes(&mut self, dest: &mut [u8]) { self.0.fill_bytes(dest) }
    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand_core::Error> {
        self.0.try_fill_bytes(dest)
    }
}

impl CryptoRng for BrokerRng {}
