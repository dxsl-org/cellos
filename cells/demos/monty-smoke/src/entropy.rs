//! Entropy for Monty's `getrandom` dependencies.
//!
//! getrandom 0.3 (via `ahash`) and 0.4 (Monty's `random`, boundary UUIDs) are
//! built with `--cfg getrandom_backend="custom"` (`.cargo/config.toml`); both
//! resolve this one symbol. Their `Error` types are each a newtype over the same
//! non-zero raw OS code, so the 0.3 signature serves both call sites.
//!
//! Fail-closed like the std PAL's PAL-019: a `GetRandom` that yields no bytes
//! is an error, never a zero-filled buffer.

#![allow(unsafe_code)]

/// Largest span one `GetRandom` call fills (the frozen ABI caps it at 64 bytes).
const CHUNK: usize = 64;

/// Fill `buf` from `source`, which writes into its argument and returns the
/// number of bytes written (0 = no entropy available).
///
/// # Errors
/// `getrandom::Error::UNSUPPORTED` as soon as `source` returns 0; `buf` is then
/// partially filled and must not be used.
pub fn fill(
    buf: &mut [u8],
    mut source: impl FnMut(&mut [u8]) -> usize,
) -> Result<(), getrandom::Error> {
    let mut offset = 0;
    while offset < buf.len() {
        let end = (offset + CHUNK).min(buf.len());
        let written = source(&mut buf[offset..end]);
        if written == 0 {
            return Err(getrandom::Error::UNSUPPORTED);
        }
        offset += written.min(end - offset);
    }
    Ok(())
}

/// The `custom` backend entry point getrandom 0.3 and 0.4 call.
///
/// # Safety
/// getrandom guarantees `dest` is valid for `len` writable bytes for the
/// duration of the call and not aliased.
#[cfg(target_os = "cellos")]
#[unsafe(no_mangle)]
unsafe extern "Rust" fn __getrandom_v03_custom(
    dest: *mut u8,
    len: usize,
) -> Result<(), getrandom::Error> {
    if len == 0 {
        return Ok(());
    }
    // SAFETY: the caller (getrandom's custom backend) passes a non-null buffer
    // valid for `len` writes that nothing else references during this call.
    let buf = unsafe { core::slice::from_raw_parts_mut(dest, len) };
    fill(buf, ostd::syscall::sys_get_random)
}

#[cfg(test)]
mod tests {
    use super::fill;

    #[test]
    fn zero_byte_source_fails_closed() {
        let mut buf = [0xAAu8; 16];
        assert!(fill(&mut buf, |_| 0).is_err());
    }

    #[test]
    fn fills_across_chunk_boundaries_with_short_writes() {
        let mut buf = [0u8; 150];
        let mut next = 1u8;
        fill(&mut buf, |chunk| {
            assert!(chunk.len() <= 64);
            // Short write: at most 40 bytes per call, as a device may return.
            let n = chunk.len().min(40);
            for b in &mut chunk[..n] {
                *b = next;
                next = next.wrapping_add(1);
            }
            n
        })
        .unwrap();
        // Sequential values with no gap or overlap: every byte written exactly once, in order.
        assert!(buf.iter().zip(1u8..).all(|(&b, want)| b == want));
    }
}
