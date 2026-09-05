#[path = "../../../cells/drivers/e1000/src/dma_layout.rs"]
mod dma_layout;

use dma_layout::{authorize_dma_layout, DmaSlot, RX_SLOTS, TX_SLOTS};

fn nonidentity_iova(slot: DmaSlot) -> u64 {
    let offset = match slot {
        DmaSlot::TxRing => 0,
        DmaSlot::TxBuffer(index) => 0x1000 * (index as u64 + 1),
        DmaSlot::RxRing => 0x20_000,
        DmaSlot::RxBuffer(index) => 0x21_000 + 0x1000 * index as u64,
    };
    0x8000_0000 + offset
}

#[test]
fn preserves_each_authorized_nonidentity_iova() {
    let layout = authorize_dma_layout::<()>(|slot| Ok(nonidentity_iova(slot)))
        .expect("all DMA slots should authorize");

    assert_eq!(layout.tx_ring, nonidentity_iova(DmaSlot::TxRing));
    assert_eq!(layout.rx_ring, nonidentity_iova(DmaSlot::RxRing));
    for index in 0..TX_SLOTS {
        assert_eq!(
            layout.tx_buffers[index],
            nonidentity_iova(DmaSlot::TxBuffer(index))
        );
    }
    for index in 0..RX_SLOTS {
        assert_eq!(
            layout.rx_buffers[index],
            nonidentity_iova(DmaSlot::RxBuffer(index))
        );
    }
}

#[test]
fn stops_at_first_authorization_rejection() {
    let rejected = DmaSlot::RxBuffer(3);
    let mut visited = Vec::new();
    let result = authorize_dma_layout(|slot| {
        visited.push(slot);
        if slot == rejected {
            Err("denied")
        } else {
            Ok(nonidentity_iova(slot))
        }
    });

    assert!(matches!(result, Err("denied")));
    assert_eq!(visited.last(), Some(&rejected));
    assert_eq!(visited.len(), 1 + TX_SLOTS + 1 + 4);
    assert!(!visited.contains(&DmaSlot::RxBuffer(4)));
}
