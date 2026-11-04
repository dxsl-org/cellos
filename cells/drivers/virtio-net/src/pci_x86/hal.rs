//! x86_64 DMA HAL: every byte the device touches lies in a Grant authorised
//! for the function's BDF through `GrantDma`, so the path holds with or
//! without VT-d translating the device.
//!
//! Invariants:
//! - `GrantDma` pins its region until the cell dies and `GrantFree` refuses a
//!   pinned region, so authorised memory is never handed back to the kernel.
//!   Ring pages released through `dma_dealloc` are parked and reused by a later
//!   `dma_alloc` of the same size; `share` bounces through one fixed pool
//!   authorised once in [`init`].
//! - Addresses given to the device are the IOVAs `GrantDma` returned; CPU
//!   accesses use the Grant's virtual address. The two are never mixed.
//! - The cell is single-threaded: HAL entry points run during `Init` or under
//!   the `STATE` lock, so the locks here never contend.

#![allow(unsafe_code)]

use crate::device::RX_BUF_LEN;
use alloc::vec::Vec;
use core::ptr::NonNull;
use core::sync::atomic::{AtomicU32, Ordering};
use ostd::dma::DmaBuf;
use ostd::sync::Mutex;
use virtio_drivers::{BufferDirection, Hal, PhysAddr, PAGE_SIZE};

/// Bytes per bounce slot. The largest shared buffer is an RX buffer
/// (`RX_BUF_LEN`); a TX buffer (12-byte header ++ frame ≤ 1514) is smaller.
const BOUNCE_SLOT: usize = 2048;
/// Slots in the pool. At most `NET_QUEUE_SIZE` (16) RX buffers are posted at
/// once, and the one synchronous TX shares a single buffer. 32 leaves headroom
/// and fits the `u32` bitmap.
const BOUNCE_SLOTS: usize = 32;
const BOUNCE_PAGES: usize = BOUNCE_SLOT * BOUNCE_SLOTS / PAGE_SIZE;

const _: () = assert!(RX_BUF_LEN <= BOUNCE_SLOT);
const _: () = assert!((BOUNCE_SLOT * BOUNCE_SLOTS).is_multiple_of(PAGE_SIZE));

/// BDF every allocation is authorised for; set once by [`init`].
static BDF: AtomicU32 = AtomicU32::new(0);

/// An authorised Grant region: `iova` for the device, `virt` for the CPU.
struct Region {
    iova: usize,
    virt: usize,
    pages: usize,
}

/// Ring regions released by the driver, still authorised, awaiting reuse.
static PARKED: Mutex<Vec<Region>> = Mutex::new(Vec::new());

/// Bounce pool; bit `i` of `used` set ⇔ slot `i` is shared with the device.
struct Bounce {
    region: Region,
    used: u32,
}

static BOUNCE: Mutex<Option<Bounce>> = Mutex::new(None);

/// Allocates `pages` Grant pages and authorises them for the bound BDF.
fn authorized(pages: usize) -> Option<Region> {
    let buf = DmaBuf::alloc(pages)?;
    match buf.authorize(BDF.load(Ordering::Acquire)) {
        Ok(iova) => Some(Region {
            iova: iova as usize,
            virt: buf.virt() as usize,
            pages,
        }),
        Err(_) => {
            buf.free();
            None
        }
    }
}

/// Binds the HAL to `bdf` and authorises the bounce pool for it. Must succeed
/// before the virtio driver is constructed; the first `GrantDma` is also what
/// enables bus mastering on the function.
pub fn init(bdf: u32) -> Result<(), &'static str> {
    BDF.store(bdf, Ordering::Release);
    let mut pool = BOUNCE.lock();
    if pool.is_none() {
        let region = authorized(BOUNCE_PAGES).ok_or("bounce pool DMA authorization failed")?;
        *pool = Some(Bounce { region, used: 0 });
    }
    Ok(())
}

/// [`Hal`] over authorised Grant regions.
pub struct PciHal;

// SAFETY: every returned region is a live, page-aligned, exclusively owned
// Grant (zeroed before return); shared buffers occupy disjoint pool slots.
unsafe impl Hal for PciHal {
    /// Returns IOVA 0 on failure, which `Dma::new` reports as `DmaError`.
    fn dma_alloc(pages: usize, _dir: BufferDirection) -> (PhysAddr, NonNull<u8>) {
        let parked = {
            let mut parked = PARKED.lock();
            let index = parked.iter().position(|r| r.pages == pages);
            index.map(|i| parked.swap_remove(i))
        };
        let Some(region) = parked.or_else(|| authorized(pages)) else {
            return (0, NonNull::dangling());
        };
        // SAFETY: the region is a Grant of `pages` pages owned by this cell and
        // not handed to the driver until this call returns.
        unsafe { core::ptr::write_bytes(region.virt as *mut u8, 0, pages * PAGE_SIZE) };
        match NonNull::new(region.virt as *mut u8) {
            Some(virt) => (region.iova, virt),
            None => (0, NonNull::dangling()),
        }
    }

    unsafe fn dma_dealloc(paddr: PhysAddr, vaddr: NonNull<u8>, pages: usize) -> i32 {
        PARKED.lock().push(Region {
            iova: paddr,
            virt: vaddr.as_ptr() as usize,
            pages,
        });
        0
    }

    unsafe fn mmio_phys_to_virt(paddr: PhysAddr, _size: usize) -> NonNull<u8> {
        // SAFETY: SAS identity mapping — the caller passes a mapped MMIO address.
        unsafe { NonNull::new_unchecked(paddr as *mut u8) }
    }

    unsafe fn share(buffer: NonNull<[u8]>, dir: BufferDirection) -> PhysAddr {
        let len = buffer.len();
        assert!(
            len <= BOUNCE_SLOT,
            "[virtio-net] {len}-byte buffer exceeds bounce slot"
        );
        let mut guard = BOUNCE.lock();
        let pool = guard
            .as_mut()
            .expect("[virtio-net] DMA HAL used before init");
        let slot = (!pool.used).trailing_zeros() as usize;
        assert!(slot < BOUNCE_SLOTS, "[virtio-net] bounce pool exhausted");
        pool.used |= 1 << slot;
        let offset = slot * BOUNCE_SLOT;
        if matches!(dir, BufferDirection::DriverToDevice | BufferDirection::Both) {
            // SAFETY: `buffer` is valid for `len` bytes (caller contract) and the
            // slot is a disjoint `BOUNCE_SLOT`-byte window of the pool.
            unsafe {
                core::ptr::copy_nonoverlapping(
                    buffer.as_ptr() as *const u8,
                    (pool.region.virt + offset) as *mut u8,
                    len,
                );
            }
        }
        pool.region.iova + offset
    }

    unsafe fn unshare(paddr: PhysAddr, buffer: NonNull<[u8]>, dir: BufferDirection) {
        let mut guard = BOUNCE.lock();
        let pool = guard
            .as_mut()
            .expect("[virtio-net] DMA HAL used before init");
        let offset = paddr.wrapping_sub(pool.region.iova);
        let slot = offset / BOUNCE_SLOT;
        assert!(
            offset.is_multiple_of(BOUNCE_SLOT)
                && slot < BOUNCE_SLOTS
                && pool.used & (1 << slot) != 0,
            "[virtio-net] unshare of an address share did not return"
        );
        if matches!(dir, BufferDirection::DeviceToDriver | BufferDirection::Both) {
            // SAFETY: the slot holds what the device wrote for this buffer, and
            // `buffer` is the same slice given to `share` (≤ BOUNCE_SLOT bytes).
            unsafe {
                core::ptr::copy_nonoverlapping(
                    (pool.region.virt + offset) as *const u8,
                    buffer.as_ptr() as *mut u8,
                    buffer.len(),
                );
            }
        }
        pool.used &= !(1 << slot);
    }
}
