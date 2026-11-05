//! Pure DMA authorization ordering and IOVA capture for the 82574 controller
//! (the same plan as `/bin/e1000`: nothing device-visible is programmed until
//! every ring and buffer has an authorized IOVA).
use core::mem::MaybeUninit;

/// Initialize a fixed-size array without heap allocation or panic-on-error.
pub(crate) fn try_init_array<T, E, const N: usize>(
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

    // SAFETY: an array of MaybeUninit<T> requires no initialization.
    let mut storage: [MaybeUninit<T>; N] = unsafe { MaybeUninit::uninit().assume_init() };
    let mut guard = InitGuard {
        storage: &mut storage,
        initialized: 0,
    };
    for index in 0..N {
        guard.storage[index].write(initialize(index)?);
        guard.initialized += 1;
    }

    // SAFETY: all N elements were initialized above. Reading transfers their
    // ownership into the returned array; clearing the count prevents double-drop.
    let initialized = unsafe { (guard.storage.as_ptr() as *const [T; N]).read() };
    guard.initialized = 0;
    Ok(initialized)
}

pub(crate) const TX_SLOTS: usize = 16;
pub(crate) const RX_SLOTS: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DmaSlot {
    TxRing,
    TxBuffer(usize),
    RxRing,
    RxBuffer(usize),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum InitialDmaProgram {
    TxRingBase(u64),
    RxDescriptor { slot: usize, iova: u64 },
    RxRingBase(u64),
    Enable,
}

#[derive(Clone, Copy)]
pub(crate) struct DmaIovas {
    tx_ring: u64,
    tx_buffers: [u64; TX_SLOTS],
    rx_ring: u64,
    rx_buffers: [u64; RX_SLOTS],
}

impl DmaIovas {
    #[inline]
    pub(crate) fn tx_ring_iova(&self) -> u64 {
        self.tx_ring
    }

    #[inline]
    pub(crate) fn rx_ring_iova(&self) -> u64 {
        self.rx_ring
    }

    #[inline]
    pub(crate) fn tx_descriptor_iova(&self, slot: usize) -> u64 {
        self.tx_buffers[slot]
    }

    #[inline]
    pub(crate) fn rx_descriptor_iova(&self, slot: usize) -> u64 {
        self.rx_buffers[slot]
    }
}

pub(crate) fn for_each_initial_dma_program(
    layout: &DmaIovas,
    mut emit: impl FnMut(InitialDmaProgram),
) {
    emit(InitialDmaProgram::TxRingBase(layout.tx_ring_iova()));
    for slot in 0..RX_SLOTS {
        emit(InitialDmaProgram::RxDescriptor {
            slot,
            iova: layout.rx_descriptor_iova(slot),
        });
    }
    emit(InitialDmaProgram::RxRingBase(layout.rx_ring_iova()));
    emit(InitialDmaProgram::Enable);
}

/// Authorize every e1000 DMA object in deterministic initialization order.
///
/// The first rejection is returned immediately, before later slots are exposed
/// to the device. Returned IOVAs are retained verbatim; callers must not replace
/// them with CPU physical addresses when programming device-visible fields.
pub(crate) fn authorize_dma_layout<E>(
    mut authorize: impl FnMut(DmaSlot) -> Result<u64, E>,
) -> Result<DmaIovas, E> {
    let tx_ring = authorize(DmaSlot::TxRing)?;
    let mut tx_buffers = [0u64; TX_SLOTS];
    for (index, iova) in tx_buffers.iter_mut().enumerate() {
        *iova = authorize(DmaSlot::TxBuffer(index))?;
    }

    let rx_ring = authorize(DmaSlot::RxRing)?;
    let mut rx_buffers = [0u64; RX_SLOTS];
    for (index, iova) in rx_buffers.iter_mut().enumerate() {
        *iova = authorize(DmaSlot::RxBuffer(index))?;
    }

    Ok(DmaIovas {
        tx_ring,
        tx_buffers,
        rx_ring,
        rx_buffers,
    })
}

/// Run DMA-address programming and TX/RX enablement only after every DMA
/// object is authorized.
///
/// Authorization rejection skips the side-effecting closure entirely.
pub(crate) fn with_authorized_dma_layout<R, E, T>(
    resources: R,
    mut authorize: impl FnMut(&R, DmaSlot) -> Result<u64, E>,
    initialize: impl FnOnce(R, DmaIovas) -> T,
) -> Result<T, E> {
    let layout = authorize_dma_layout(|slot| authorize(&resources, slot))?;
    Ok(initialize(resources, layout))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejection_skips_programming_and_iovas_are_kept_verbatim() {
        let mut programs = Vec::new();
        let denied = with_authorized_dma_layout(
            (),
            |_, slot| {
                if slot == DmaSlot::RxBuffer(3) {
                    Err(())
                } else {
                    Ok(0x1000)
                }
            },
            |(), layout| for_each_initial_dma_program(&layout, |p| programs.push(p)),
        );
        assert_eq!(denied, Err(()));
        assert!(programs.is_empty());

        let iova = |slot| match slot {
            DmaSlot::TxRing => 0xA000,
            DmaSlot::TxBuffer(i) => 0xB000 + i as u64 * 0x1000,
            DmaSlot::RxRing => 0xC000,
            DmaSlot::RxBuffer(i) => 0xD000 + i as u64 * 0x1000,
        };
        let layout = authorize_dma_layout::<()>(|slot| Ok(iova(slot))).unwrap();
        for_each_initial_dma_program(&layout, |p| programs.push(p));
        assert_eq!(
            programs.first(),
            Some(&InitialDmaProgram::TxRingBase(0xA000))
        );
        assert_eq!(programs.last(), Some(&InitialDmaProgram::Enable));
        assert_eq!(programs.len(), RX_SLOTS + 3);
        assert_eq!(layout.tx_descriptor_iova(2), 0xD000);
        assert_eq!(layout.rx_descriptor_iova(1), 0xE000);
        assert_eq!(
            (layout.tx_ring_iova(), layout.rx_ring_iova()),
            (0xA000, 0xC000)
        );
    }
}
