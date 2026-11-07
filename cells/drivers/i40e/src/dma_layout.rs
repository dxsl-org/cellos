//! DMA layout and IOVA authorization ordering for Intel i40e.

use core::mem::MaybeUninit;

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
                unsafe { value.assume_init_drop() };
            }
        }
    }

    let mut storage: [MaybeUninit<T>; N] = unsafe { MaybeUninit::uninit().assume_init() };
    let mut guard = InitGuard {
        storage: &mut storage,
        initialized: 0,
    };
    for index in 0..N {
        guard.storage[index].write(initialize(index)?);
        guard.initialized += 1;
    }
    let array = unsafe { (guard.storage.as_ptr() as *const [T; N]).read() };
    guard.initialized = 0;
    Ok(array)
}

pub(crate) const TX_SLOTS: usize = 16;
pub(crate) const RX_SLOTS: usize = 16;
pub(crate) const AQ_SLOTS: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DmaSlot {
    AtqRing,
    ArqRing,
    AqBuffer,
    TxRing,
    TxBuffer(usize),
    RxRing,
    RxBuffer(usize),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum InitialDmaProgram {
    AtqBase(u64),
    ArqBase(u64),
    TxRingBase(u64),
    RxRingBase(u64),
    RxBuffer { index: usize, iova: u64 },
}

#[derive(Clone, Copy)]
pub(crate) struct DmaIovas {
    pub(crate) atq_ring: u64,
    pub(crate) arq_ring: u64,
    pub(crate) aq_buffer: u64,
    pub(crate) tx_ring: u64,
    pub(crate) rx_ring: u64,
    pub(crate) tx_bufs: [u64; TX_SLOTS],
    pub(crate) rx_bufs: [u64; RX_SLOTS],
}

pub(crate) fn for_each_initial_dma_program(
    layout: &DmaIovas,
    mut emit: impl FnMut(InitialDmaProgram),
) {
    emit(InitialDmaProgram::AtqBase(layout.atq_ring));
    emit(InitialDmaProgram::ArqBase(layout.arq_ring));
    emit(InitialDmaProgram::TxRingBase(layout.tx_ring));
    emit(InitialDmaProgram::RxRingBase(layout.rx_ring));
    for (index, &iova) in layout.rx_bufs.iter().enumerate() {
        emit(InitialDmaProgram::RxBuffer { index, iova });
    }
}

pub(crate) fn authorize_dma_layout<E>(
    mut authorize: impl FnMut(DmaSlot) -> Result<u64, E>,
) -> Result<DmaIovas, E> {
    let atq_ring = authorize(DmaSlot::AtqRing)?;
    let arq_ring = authorize(DmaSlot::ArqRing)?;
    let aq_buffer = authorize(DmaSlot::AqBuffer)?;
    let tx_ring = authorize(DmaSlot::TxRing)?;
    let mut tx_bufs = [0u64; TX_SLOTS];
    for (index, slot) in tx_bufs.iter_mut().enumerate() {
        *slot = authorize(DmaSlot::TxBuffer(index))?;
    }
    let rx_ring = authorize(DmaSlot::RxRing)?;
    let mut rx_bufs = [0u64; RX_SLOTS];
    for (index, slot) in rx_bufs.iter_mut().enumerate() {
        *slot = authorize(DmaSlot::RxBuffer(index))?;
    }
    Ok(DmaIovas {
        atq_ring,
        arq_ring,
        aq_buffer,
        tx_ring,
        rx_ring,
        tx_bufs,
        rx_bufs,
    })
}

pub(crate) fn with_authorized_dma_layout<R, E, T>(
    resources: R,
    mut authorize: impl FnMut(&R, DmaSlot) -> Result<u64, E>,
    initialize: impl FnOnce(R, DmaIovas) -> T,
) -> Result<T, E> {
    let layout = authorize_dma_layout(|slot| authorize(&resources, slot))?;
    Ok(initialize(resources, layout))
}
