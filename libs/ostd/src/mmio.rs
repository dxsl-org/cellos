//! Safe MMIO accessor for Driver Cells.
//!
//! Driver Cells use `#![forbid(unsafe_code)]` and cannot directly deref raw
//! hardware addresses.  `MmioRegion` hides the `unsafe` inside this trusted
//! library and enforces bounds checking on every access.
//!
//! A Cell cannot construct `MmioRegion` directly — the constructor is
//! `pub(crate)`.  Cells obtain one by calling `request_region()`, which
//! goes through the kernel Resource Registry syscall (Phase 02).

use crate::syscall;
use types::{ViError, ViResult};

/// Request exclusive access to the MMIO range `[base, base+len)` from the kernel.
///
/// On success, returns an `MmioRegion` the Driver Cell can use for volatile
/// register reads and writes.  The kernel:
/// 1. Checks `[base, base+len)` against the per-arch allowlist.
/// 2. Verifies no other Cell currently owns the range.
/// 3. Checks the caller's ELF manifest declared the matching peripheral cap.
///
/// # Errors
/// - `PermissionDenied` — range not in allowlist or manifest cap missing.
/// - `AlreadyExists` — another Cell owns an overlapping range.
/// - `InvalidInput` — arithmetic overflow in `base + len`.
pub fn request_region(base: usize, len: usize) -> ViResult<MmioRegion> {
    let ret = syscall::sys_request_mmio(base, len);
    if ret == 0 {
        // SAFETY: kernel approved the range; it is valid device MMIO.
        Ok(MmioRegion::new(base, len))
    } else {
        Err(match ret {
            1 => ViError::PermissionDenied,
            2 => ViError::AlreadyExists,
            _ => ViError::InvalidInput,
        })
    }
}

mod sealed {
    pub trait Sealed {}
    impl Sealed for u8 {}
    impl Sealed for u16 {}
    impl Sealed for u32 {}
    impl Sealed for u64 {}
}

/// A device register width [`MmioRegion`] reads and writes: `u8`, `u16`, `u32`
/// or `u64`.
///
/// Sealed: every bit pattern a device returns is a valid value of each of
/// these, so a `forbid(unsafe_code)` Cell cannot turn device bytes into an
/// invalid `bool`, reference or function pointer through this accessor.
///
/// ```compile_fail
/// fn probe(region: &ostd::mmio::MmioRegion) -> ostd::ViResult<bool> {
///     region.read::<bool>(0)
/// }
/// ```
///
/// ```
/// fn probe(region: &ostd::mmio::MmioRegion) -> ostd::ViResult<u64> {
///     region.read::<u64>(0)
/// }
/// ```
pub trait MmioValue: sealed::Sealed + Copy {}
impl MmioValue for u8 {}
impl MmioValue for u16 {}
impl MmioValue for u32 {}
impl MmioValue for u64 {}

/// A bounds-checked, volatile read/write accessor for a device MMIO window.
///
/// Fields are private; Cell code (a separate Rust crate) cannot forge an
/// `MmioRegion`.  Obtain one via [`request_region`].
pub struct MmioRegion {
    base: usize,
    len: usize,
}

impl MmioRegion {
    /// Construct an `MmioRegion` for `[base, base+len)`.
    ///
    /// `pub(crate)` — only ostd internals (i.e. `request_region`) call this
    /// after the kernel has confirmed exclusive access for the calling Cell.
    pub(crate) fn new(base: usize, len: usize) -> Self {
        Self { base, len }
    }

    /// The address of a `T` at `offset`: inside the region and aligned for
    /// `T`, or `Err(InvalidInput)`.
    fn register<T: MmioValue>(&self, offset: usize) -> ViResult<usize> {
        let end = offset
            .checked_add(core::mem::size_of::<T>())
            .ok_or(ViError::InvalidInput)?;
        if end > self.len {
            return Err(ViError::InvalidInput);
        }
        let address = self.base + offset;
        if !address.is_multiple_of(core::mem::align_of::<T>()) {
            return Err(ViError::InvalidInput);
        }
        Ok(address)
    }

    /// Read a `T`-sized register at `offset` bytes from the region base.
    ///
    /// Returns `Err(InvalidInput)` if `offset + size_of::<T>()` would exceed
    /// `self.len`, if the arithmetic overflows, or if the register address is
    /// not aligned for `T`.
    pub fn read<T: MmioValue>(&self, offset: usize) -> ViResult<T> {
        let ptr = self.register::<T>(offset)? as *const T;
        // SAFETY: `register` holds [ptr, ptr+size) inside [base, base+len) and
        // `ptr` aligned for `T`; every bit pattern is a valid `T` (`MmioValue`).
        // The Region was granted by the kernel Resource Registry, guaranteeing
        // the address range is valid device MMIO.  `read_volatile` prevents the
        // compiler from caching or eliding the hardware read.
        Ok(unsafe { core::ptr::read_volatile(ptr) })
    }

    /// Write `val` of type `T` at `offset` bytes from the region base.
    ///
    /// Returns `Err(InvalidInput)` when out of bounds or misaligned.
    pub fn write<T: MmioValue>(&self, offset: usize, val: T) -> ViResult<()> {
        let ptr = self.register::<T>(offset)? as *mut T;
        // SAFETY: same contract as `read` above.
        unsafe { core::ptr::write_volatile(ptr, val) };
        Ok(())
    }

    /// Read a 32-bit device register at `offset` (most common register width).
    #[inline]
    pub fn read_u32(&self, offset: usize) -> ViResult<u32> {
        self.read::<u32>(offset)
    }

    /// Write a 32-bit device register at `offset`.
    #[inline]
    pub fn write_u32(&self, offset: usize, val: u32) -> ViResult<()> {
        self.write::<u32>(offset, val)
    }

    /// Read an 8-bit device register at `offset`.
    #[inline]
    pub fn read_u8(&self, offset: usize) -> ViResult<u8> {
        self.read::<u8>(offset)
    }

    /// Write an 8-bit device register at `offset`.
    #[inline]
    pub fn write_u8(&self, offset: usize, val: u8) -> ViResult<()> {
        self.write::<u8>(offset, val)
    }

    /// Base address of this region (physical/MMIO address).
    #[inline]
    pub fn base(&self) -> usize {
        self.base
    }

    /// Length of this region in bytes.
    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns `true` if the region has zero length.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: back an MmioRegion of `len` bytes with an 8-aligned stack buffer.
    fn make_region(buf: &mut [u64], len: usize) -> MmioRegion {
        assert!(len <= buf.len() * 8);
        MmioRegion::new(buf.as_mut_ptr() as usize, len)
    }

    #[test]
    fn rejects_oob_read() {
        let mut buf = [0u64; 1];
        let r = make_region(&mut buf, 4);
        // offset=0, size=4 → end=4 == len=4 → OK
        assert!(r.read_u32(0).is_ok());
        // offset=1, size=4 → end=5 > len=4 → Err
        assert_eq!(r.read_u32(1), Err(ViError::InvalidInput));
        // offset=4, size=4 → end=8 > len=4 → Err
        assert_eq!(r.read_u32(4), Err(ViError::InvalidInput));
    }

    #[test]
    fn rejects_oob_write() {
        let mut buf = [0u64; 1];
        let r = make_region(&mut buf, 4);
        assert!(r.write_u32(0, 0x1234_5678).is_ok());
        assert_eq!(r.write_u32(1, 0), Err(ViError::InvalidInput));
    }

    /// 10 S-16: a misaligned register access is refused, never performed.
    #[test]
    fn rejects_misaligned_access() {
        let mut buf = [0u64; 2];
        let r = make_region(&mut buf, 16);
        assert_eq!(r.read::<u16>(1), Err(ViError::InvalidInput));
        assert_eq!(r.read_u32(2), Err(ViError::InvalidInput));
        assert_eq!(r.read::<u64>(4), Err(ViError::InvalidInput));
        assert_eq!(r.write::<u64>(12, 0), Err(ViError::InvalidInput));
        assert_eq!(r.write::<u16>(3, 0), Err(ViError::InvalidInput));
        // Naturally aligned accesses of every width still work.
        r.write::<u64>(8, 0x0102_0304_0506_0708).unwrap();
        assert_eq!(r.read::<u64>(8), Ok(0x0102_0304_0506_0708));
        assert_eq!(r.read::<u16>(2), Ok(0));
        assert_eq!(
            r.read_u8(15),
            Ok(u64::to_ne_bytes(0x0102_0304_0506_0708)[7])
        );
    }

    #[test]
    fn round_trip_u32() {
        let mut buf = [0u64; 2];
        let r = make_region(&mut buf, 16);
        r.write_u32(0, 0xDEAD_BEEF).unwrap();
        assert_eq!(r.read_u32(0).unwrap(), 0xDEAD_BEEF);
        r.write_u32(4, 0x1234_5678).unwrap();
        assert_eq!(r.read_u32(4).unwrap(), 0x1234_5678);
        // independent offsets do not clobber each other
        assert_eq!(r.read_u32(0).unwrap(), 0xDEAD_BEEF);
    }

    #[test]
    fn round_trip_u8() {
        let mut buf = [0u64; 1];
        let r = make_region(&mut buf, 4);
        r.write_u8(0, 0xAB).unwrap();
        r.write_u8(3, 0xCD).unwrap();
        assert_eq!(r.read_u8(0).unwrap(), 0xAB);
        assert_eq!(r.read_u8(3).unwrap(), 0xCD);
    }

    #[test]
    fn overflow_offset_rejected() {
        let mut buf = [0u64; 1];
        let r = make_region(&mut buf, 4);
        // offset near usize::MAX + size_of::<u32>()=4 must not overflow
        assert_eq!(r.read_u32(usize::MAX - 2), Err(ViError::InvalidInput));
        assert_eq!(r.write_u32(usize::MAX, 0), Err(ViError::InvalidInput));
    }
}
