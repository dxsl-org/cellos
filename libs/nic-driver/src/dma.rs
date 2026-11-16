//! Authorize-before-program DMA layout.
//!
//! Every NIC cell allocates its rings and frame buffers, authorizes each one
//! for its BDF (`DmaBuf::authorize` → `GrantDma`) in one deterministic order,
//! and only then programs any device-visible address. The first refusal stops
//! the walk before a later slot is exposed, and the returned IOVAs are kept
//! verbatim: a chip must never substitute a CPU physical address for them.
//!
//! The order is: extra slots placed [`Position::Before`] (i40e's admin
//! queues), TX ring, TX buffers, RX ring, RX buffers, extra slots placed
//! [`Position::After`] (bcm57xx's status block). The chip programs its own
//! registers from the returned [`Iovas`].

use crate::ring::{self, DescRing, FrameBufs};
use core::mem::MaybeUninit;
use ostd::dma::DmaBuf;
use types::{ViError, ViResult};

/// Initialize a fixed-size array without heap allocation or panic-on-error.
/// On the first error, the already-built prefix is dropped and the error is
/// returned.
pub fn try_init_array<T, E, const N: usize>(
    mut initialize: impl FnMut(usize) -> Result<T, E>,
) -> Result<[T; N], E> {
    struct InitGuard<'a, T> {
        storage: &'a mut [MaybeUninit<T>],
        initialized: usize,
    }

    impl<T> Drop for InitGuard<'_, T> {
        fn drop(&mut self) {
            for value in &mut self.storage[..self.initialized] {
                // SAFETY: only the prefix counted by `initialized` was written.
                unsafe { value.assume_init_drop() };
            }
        }
    }

    let mut storage: [MaybeUninit<T>; N] = [const { MaybeUninit::uninit() }; N];
    let mut guard = InitGuard {
        storage: &mut storage,
        initialized: 0,
    };
    for index in 0..N {
        guard.storage[index].write(initialize(index)?);
        guard.initialized += 1;
    }

    // SAFETY: all N elements were initialized above. Reading transfers their
    // ownership into the returned array; clearing the count prevents a double
    // drop when the guard goes out of scope.
    let initialized = unsafe { (guard.storage.as_ptr() as *const [T; N]).read() };
    guard.initialized = 0;
    Ok(initialized)
}

/// Where a chip's extra DMA slot is authorized relative to the rings.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Position {
    Before,
    After,
}

/// One authorized DMA object.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Slot {
    /// The chip's extra slot `i`, in declaration order.
    Extra(usize),
    TxRing,
    TxBuffer(usize),
    RxRing,
    RxBuffer(usize),
}

/// Device-visible addresses returned by authorization.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Iovas<const TX: usize, const RX: usize, const E: usize> {
    pub tx_ring: u64,
    pub tx_bufs: [u64; TX],
    pub rx_ring: u64,
    pub rx_bufs: [u64; RX],
    pub extra: [u64; E],
}

/// A chip's DMA slot order: the rings and buffers, plus `E` extra slots.
pub struct Layout<const E: usize> {
    extras: [Position; E],
}

impl Layout<0> {
    /// Rings and buffers only.
    pub const PLAIN: Self = Self { extras: [] };
}

impl<const E: usize> Layout<E> {
    pub const fn new(extras: [Position; E]) -> Self {
        Self { extras }
    }

    /// Authorize every slot in layout order. The first refusal is returned
    /// at once; later slots are never offered.
    pub fn authorize<const TX: usize, const RX: usize, Er>(
        &self,
        mut authorize: impl FnMut(Slot) -> Result<u64, Er>,
    ) -> Result<Iovas<TX, RX, E>, Er> {
        let mut extra = [0u64; E];
        self.extras_at(Position::Before, &mut extra, &mut authorize)?;
        let tx_ring = authorize(Slot::TxRing)?;
        let mut tx_bufs = [0u64; TX];
        for (index, iova) in tx_bufs.iter_mut().enumerate() {
            *iova = authorize(Slot::TxBuffer(index))?;
        }
        let rx_ring = authorize(Slot::RxRing)?;
        let mut rx_bufs = [0u64; RX];
        for (index, iova) in rx_bufs.iter_mut().enumerate() {
            *iova = authorize(Slot::RxBuffer(index))?;
        }
        self.extras_at(Position::After, &mut extra, &mut authorize)?;
        Ok(Iovas {
            tx_ring,
            tx_bufs,
            rx_ring,
            rx_bufs,
            extra,
        })
    }

    fn extras_at<Er>(
        &self,
        position: Position,
        extra: &mut [u64; E],
        authorize: &mut impl FnMut(Slot) -> Result<u64, Er>,
    ) -> Result<(), Er> {
        for (index, iova) in extra.iter_mut().enumerate() {
            if self.extras[index] == position {
                *iova = authorize(Slot::Extra(index))?;
            }
        }
        Ok(())
    }
}

/// A chip's rings, frame buffers and extra pages, every one authorized for
/// the controller and the rings and extra pages zeroed. Nothing has been
/// programmed into the device yet.
pub struct DmaSet<T: Copy, R: Copy, const TX: usize, const RX: usize, const E: usize> {
    pub tx_ring: DescRing<T, TX>,
    pub tx_bufs: FrameBufs<TX>,
    pub rx_ring: DescRing<R, RX>,
    pub rx_bufs: FrameBufs<RX>,
    pub extra: [DmaBuf; E],
    pub iovas: Iovas<TX, RX, E>,
}

impl<T: Copy, R: Copy, const TX: usize, const RX: usize, const E: usize> DmaSet<T, R, TX, RX, E> {
    /// Allocate one page per ring, buffer and extra slot, then authorize them
    /// all for `bdf` in `layout` order. `OutOfMemory` on an allocation
    /// failure, `PermissionDenied` on the first refused authorization.
    ///
    /// On failure the pages are not freed (`DmaBuf` has no drop cleanup):
    /// the cell exits and task teardown reclaims its grants.
    pub fn alloc_authorized(layout: &Layout<E>, bdf: u32) -> ViResult<Self> {
        let page = || DmaBuf::alloc(1).ok_or(ViError::OutOfMemory);
        let extra: [DmaBuf; E] = try_init_array(|_| page())?;
        let tx_ring = DescRing::new(page()?)?;
        let tx_bufs = FrameBufs::alloc()?;
        let rx_ring = DescRing::new(page()?)?;
        let rx_bufs = FrameBufs::alloc()?;

        let iovas = layout.authorize(|slot| {
            let buf = match slot {
                Slot::Extra(index) => &extra[index],
                Slot::TxRing => tx_ring.dma(),
                Slot::TxBuffer(index) => tx_bufs.dma(index),
                Slot::RxRing => rx_ring.dma(),
                Slot::RxBuffer(index) => rx_bufs.dma(index),
            };
            buf.authorize(bdf).map_err(|_| ViError::PermissionDenied)
        })?;

        tx_ring.clear();
        rx_ring.clear();
        for buf in &extra {
            ring::zero(buf);
        }
        Ok(Self {
            tx_ring,
            tx_bufs,
            rx_ring,
            rx_bufs,
            extra,
            iovas,
        })
    }
}

#[cfg(test)]
mod tests {
    // The `e1000-dma-layout` integration cases, moved here with the layout
    // they test (Phase 18).
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    const TX: usize = 16;
    const RX: usize = 16;

    fn nonidentity_iova(slot: Slot) -> u64 {
        let offset = match slot {
            Slot::TxRing => 0,
            Slot::TxBuffer(index) => 0x1000 * (index as u64 + 1),
            Slot::RxRing => 0x20_000,
            Slot::RxBuffer(index) => 0x21_000 + 0x1000 * index as u64,
            Slot::Extra(index) => 0x40_000 + 0x1000 * index as u64,
        };
        0x8000_0000 + offset
    }

    fn order<const E: usize>(layout: &Layout<E>) -> Vec<Slot> {
        let mut visited = Vec::new();
        layout
            .authorize::<TX, RX, ()>(|slot| {
                visited.push(slot);
                Ok(0)
            })
            .unwrap();
        visited
    }

    fn rings() -> Vec<Slot> {
        let mut slots = vec![Slot::TxRing];
        slots.extend((0..TX).map(Slot::TxBuffer));
        slots.push(Slot::RxRing);
        slots.extend((0..RX).map(Slot::RxBuffer));
        slots
    }

    #[test]
    fn preserves_each_authorized_nonidentity_iova() {
        let iovas = Layout::PLAIN
            .authorize::<TX, RX, ()>(|slot| Ok(nonidentity_iova(slot)))
            .expect("all DMA slots should authorize");

        assert_eq!(iovas.tx_ring, nonidentity_iova(Slot::TxRing));
        assert_eq!(iovas.rx_ring, nonidentity_iova(Slot::RxRing));
        for index in 0..TX {
            assert_eq!(
                iovas.tx_bufs[index],
                nonidentity_iova(Slot::TxBuffer(index))
            );
        }
        for index in 0..RX {
            assert_eq!(
                iovas.rx_bufs[index],
                nonidentity_iova(Slot::RxBuffer(index))
            );
        }
    }

    #[test]
    fn stops_at_first_authorization_rejection() {
        let rejected = Slot::RxBuffer(3);
        let mut visited = Vec::new();
        let result = Layout::PLAIN.authorize::<TX, RX, _>(|slot| {
            visited.push(slot);
            if slot == rejected {
                Err("denied")
            } else {
                Ok(nonidentity_iova(slot))
            }
        });

        assert!(matches!(result, Err("denied")));
        assert_eq!(visited.last(), Some(&rejected));
        assert_eq!(visited.len(), 1 + TX + 1 + 4);
        assert!(!visited.contains(&Slot::RxBuffer(4)));
    }

    #[test]
    fn rings_authorize_tx_then_rx() {
        assert_eq!(order(&Layout::PLAIN), rings());
    }

    #[test]
    fn extra_slots_keep_the_i40e_and_bcm57xx_orders() {
        // i40e: admin send queue, admin receive queue, admin buffer, then rings.
        let i40e = Layout::new([Position::Before; 3]);
        let mut expected = vec![Slot::Extra(0), Slot::Extra(1), Slot::Extra(2)];
        expected.extend(rings());
        assert_eq!(order(&i40e), expected);

        // bcm57xx: rings, then the status block.
        let bcm = Layout::new([Position::After]);
        let mut expected = rings();
        expected.push(Slot::Extra(0));
        assert_eq!(order(&bcm), expected);

        let iovas = bcm
            .authorize::<TX, RX, ()>(|slot| Ok(nonidentity_iova(slot)))
            .unwrap();
        assert_eq!(iovas.extra, [nonidentity_iova(Slot::Extra(0))]);
    }

    #[test]
    fn rejected_extra_slot_exposes_no_ring() {
        let mut visited = Vec::new();
        let result =
            Layout::new([Position::Before, Position::Before]).authorize::<TX, RX, _>(|slot| {
                visited.push(slot);
                if slot == Slot::Extra(1) {
                    Err("denied")
                } else {
                    Ok(0)
                }
            });
        assert!(matches!(result, Err("denied")));
        assert_eq!(visited, [Slot::Extra(0), Slot::Extra(1)]);
    }

    /// A chip programs addresses only from `Iovas`, which exists only after
    /// every slot was authorized; none of the migrated cells may reach for
    /// the CPU physical address instead.
    #[test]
    fn nic_cells_cannot_fall_back_to_cpu_physical_addresses() {
        let cells = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../cells/drivers");
        for cell in [
            "igc", "ixgbe", "i40e", "r8169", "bcm57xx", "e1000", "e1000e", "igb", "vmxnet3",
        ] {
            let src = cells.join(cell).join("src");
            for entry in std::fs::read_dir(&src).expect("cell source directory") {
                let path = entry.unwrap().path();
                let text = std::fs::read_to_string(&path).unwrap();
                assert!(
                    !text.contains(".phys()"),
                    "{} substitutes a CPU physical address for an authorized IOVA",
                    path.display()
                );
            }
        }
    }

    struct DropProbe(Rc<Cell<usize>>);

    impl Drop for DropProbe {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }

    #[test]
    fn fallible_array_initializes_every_slot_without_heap_staging() {
        let drops = Rc::new(Cell::new(0));
        let values = try_init_array::<_, (), 4>(|_| Ok(DropProbe(Rc::clone(&drops))))
            .expect("all slots should initialize");

        assert_eq!(values.len(), 4);
        assert_eq!(drops.get(), 0);
        drop(values);
        assert_eq!(drops.get(), 4);
    }

    #[test]
    fn fallible_array_stops_and_drops_initialized_prefix_on_error() {
        let drops = Rc::new(Cell::new(0));
        let attempts = Cell::new(0);
        let result: Result<[DropProbe; 8], &str> = try_init_array(|index| {
            attempts.set(attempts.get() + 1);
            if index == 3 {
                Err("out of memory")
            } else {
                Ok(DropProbe(Rc::clone(&drops)))
            }
        });

        assert!(matches!(result, Err("out of memory")));
        assert_eq!(attempts.get(), 4);
        assert_eq!(drops.get(), 3);
    }
}
