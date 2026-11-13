//! x86_64 DMA HAL: every byte the device can reach is authorised for the
//! function's BDF through `GrantDma`, so DMA works behind VT-d (the returned
//! IOVA is what the device is given) and in identity mode alike.
//!
//! # Pinning contract
//! `GrantDma` pins a grant until task teardown: the kernel refuses `GrantFree`
//! on it and only revokes the IOMMU mapping and reclaims the frames when the
//! cell exits. Allocation is therefore one-shot:
//! - virtqueue rings come from `dma_alloc`, authorised once each; `dma_dealloc`
//!   leaves them to teardown.
//! - request buffers bounce through [`BOUNCE_SLOTS`] one-page slots authorised
//!   by [`prepare`] before the device is built; `share` / `unshare` recycle the
//!   slots, so steady-state I/O makes no syscalls.
//!
//! # Unsafe island
//! `virtio_drivers::Hal` is an `unsafe trait`, and bouncing copies through raw
//! grant pointers.

#![allow(unsafe_code)]

use core::ptr::NonNull;
use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use ostd::dma::DmaBuf;
use ostd::syscall::SyscallError;
use virtio_drivers::{BufferDirection, Hal, PhysAddr, PAGE_SIZE};

/// Bounce slots. One request shares at most four buffers at once: the request
/// header, the data, the status byte and — because `RING_INDIRECT_DESC` is
/// negotiated — the indirect descriptor table. Requests are serialised by the
/// cell's state lock (one in flight; after a stall the device is poisoned and
/// its slots stay claimed), so six slots leave two spare.
const BOUNCE_SLOTS: usize = 6;

/// Bytes per bounce slot; larger buffers are a caller contract violation.
/// Dispatch's largest buffer is a whole `OP_READ_RUN`, checked below.
const SLOT_BYTES: usize = PAGE_SIZE;
const _: () = assert!(
    types::block_wire::MAX_RUN_SECTORS as usize * types::block_wire::SECTOR_SIZE <= SLOT_BYTES
);

/// BDF every allocation is authorised for; stored by [`prepare`].
static BDF: AtomicU32 = AtomicU32::new(0);
/// CPU address of the bounce pool (0 until [`prepare`] succeeds).
static POOL_VIRT: AtomicUsize = AtomicUsize::new(0);
/// Device-visible address of the bounce pool, as returned by `GrantDma`.
static POOL_IOVA: AtomicUsize = AtomicUsize::new(0);
/// Bit `n` set while bounce slot `n` is shared with the device.
static SLOTS_BUSY: AtomicU32 = AtomicU32::new(0);

/// Allocates and authorises `pages` zeroed pages for `bdf`.
/// Returns `(iova, cpu address)`; the grant is freed if authorisation fails.
fn alloc_authorized(bdf: u32, pages: usize) -> Result<(PhysAddr, NonNull<u8>), SyscallError> {
    let buf = DmaBuf::alloc(pages).ok_or(SyscallError::OutOfMemory)?;
    let iova = match buf.authorize(bdf) {
        Ok(iova) => iova as PhysAddr,
        Err(error) => {
            buf.free();
            return Err(error);
        }
    };
    let virt = NonNull::new(buf.virt()).ok_or(SyscallError::OutOfMemory)?;
    // SAFETY: the grant is `buf.size()` bytes of identity-mapped memory this
    // cell owns; the device is not yet using it.
    unsafe { virt.as_ptr().write_bytes(0, buf.size()) };
    Ok((iova, virt))
}

/// Binds the HAL to `bdf` and authorises the bounce pool. Must succeed before
/// any virtio-drivers call on the device; the BDF reaches the static `Hal`
/// functions only through this call.
pub fn prepare(bdf: u32) -> Result<(), SyscallError> {
    BDF.store(bdf, Ordering::Release);
    let (iova, virt) = alloc_authorized(bdf, BOUNCE_SLOTS * SLOT_BYTES / PAGE_SIZE)?;
    POOL_IOVA.store(iova, Ordering::Release);
    POOL_VIRT.store(virt.as_ptr() as usize, Ordering::Release);
    Ok(())
}

/// Claims a free bounce slot. Exhaustion breaks the [`BOUNCE_SLOTS`]
/// invariant and fails closed: handing the device an unauthorised address
/// is never an option.
fn claim_slot() -> usize {
    let mut busy = SLOTS_BUSY.load(Ordering::Acquire);
    loop {
        let slot = (!busy).trailing_zeros() as usize;
        assert!(slot < BOUNCE_SLOTS, "[virtio-blk] bounce slots exhausted");
        match SLOTS_BUSY.compare_exchange(
            busy,
            busy | (1 << slot),
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => return slot,
            Err(current) => busy = current,
        }
    }
}

fn slot_virt(slot: usize) -> *mut u8 {
    (POOL_VIRT.load(Ordering::Acquire) + slot * SLOT_BYTES) as *mut u8
}

/// VirtIO DMA HAL for a PCI function behind (optional) DMA remapping.
pub(crate) struct CellHal;

// SAFETY: allocations are page-aligned, zeroed, exclusively owned grants that
// stay mapped until task teardown; `share` returns an authorised slot address
// that is not reused until the matching `unshare`.
unsafe impl Hal for CellHal {
    /// Returns `(0, dangling)` on failure, which virtio-drivers reports as
    /// `Error::DmaError` from the device constructor.
    fn dma_alloc(pages: usize, _dir: BufferDirection) -> (PhysAddr, NonNull<u8>) {
        alloc_authorized(BDF.load(Ordering::Acquire), pages).unwrap_or((0, NonNull::dangling()))
    }

    /// Pinned by `GrantDma`: the frames are reclaimed at task teardown.
    unsafe fn dma_dealloc(_paddr: PhysAddr, _vaddr: NonNull<u8>, _pages: usize) -> i32 {
        0
    }

    unsafe fn mmio_phys_to_virt(paddr: PhysAddr, _size: usize) -> NonNull<u8> {
        // SAFETY: SAS identity mapping. The PCI transport maps BARs through its
        // `BarMapper` and never calls this.
        unsafe { NonNull::new_unchecked(paddr as *mut u8) }
    }

    unsafe fn share(buffer: NonNull<[u8]>, dir: BufferDirection) -> PhysAddr {
        let len = buffer.len();
        assert!(
            len <= SLOT_BYTES,
            "[virtio-blk] DMA buffer of {len} bytes exceeds a slot"
        );
        let slot = claim_slot();
        if matches!(dir, BufferDirection::DriverToDevice | BufferDirection::Both) {
            // SAFETY: buffer is live for the call; the slot is SLOT_BYTES >= len
            // bytes of the pool and exclusively ours until unshare.
            unsafe {
                core::ptr::copy_nonoverlapping(buffer.as_ptr() as *const u8, slot_virt(slot), len);
            }
        }
        POOL_IOVA.load(Ordering::Acquire) + slot * SLOT_BYTES
    }

    unsafe fn unshare(paddr: PhysAddr, buffer: NonNull<[u8]>, dir: BufferDirection) {
        let slot = (paddr - POOL_IOVA.load(Ordering::Acquire)) / SLOT_BYTES;
        if matches!(dir, BufferDirection::DeviceToDriver | BufferDirection::Both) {
            // SAFETY: paddr came from share(), so `slot` is the slot holding the
            // device's bytes; buffer is the same slice, valid for its length.
            unsafe {
                core::ptr::copy_nonoverlapping(
                    slot_virt(slot),
                    buffer.as_ptr() as *mut u8,
                    buffer.len(),
                );
            }
        }
        SLOTS_BUSY.fetch_and(!(1 << slot), Ordering::Release);
    }
}
