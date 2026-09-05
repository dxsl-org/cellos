//! Pure DMA authorization ordering and IOVA capture for the e1000 controller.

pub(crate) const TX_SLOTS: usize = 16;
pub(crate) const RX_SLOTS: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DmaSlot {
    TxRing,
    TxBuffer(usize),
    RxRing,
    RxBuffer(usize),
}

pub(crate) struct DmaIovas {
    pub(crate) tx_ring: u64,
    pub(crate) tx_buffers: [u64; TX_SLOTS],
    pub(crate) rx_ring: u64,
    pub(crate) rx_buffers: [u64; RX_SLOTS],
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
