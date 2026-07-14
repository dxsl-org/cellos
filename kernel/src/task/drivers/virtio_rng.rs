/// VirtIO RNG stub — full MMIO probe deferred until a safe slot-skip strategy
/// is in place (probing already-claimed block/net slots hangs on RISC-V).
///
/// `get_random` returns 0 bytes; callers (e.g. sys_get_random) handle the
/// zero-bytes case gracefully and fall back to a PRNG seeded from the timer.
pub fn init_driver() {}

pub fn get_random(_buf: &mut [u8]) -> usize {
    0
}
