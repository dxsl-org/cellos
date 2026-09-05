#[path = "../../../cells/drivers/e1000/src/dma_layout.rs"]
mod dma_layout;

use dma_layout::{
    authorize_dma_layout, for_each_initial_dma_program, with_authorized_dma_layout, DmaSlot,
    InitialDmaProgram, RX_SLOTS, TX_SLOTS,
};

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

    assert_eq!(layout.tx_ring_iova(), nonidentity_iova(DmaSlot::TxRing));
    assert_eq!(layout.rx_ring_iova(), nonidentity_iova(DmaSlot::RxRing));
    for index in 0..TX_SLOTS {
        assert_eq!(
            layout.tx_descriptor_iova(index),
            nonidentity_iova(DmaSlot::TxBuffer(index))
        );
    }
    for index in 0..RX_SLOTS {
        assert_eq!(
            layout.rx_descriptor_iova(index),
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

#[test]
fn nonidentity_iovas_reach_each_programming_output() {
    let outputs = with_authorized_dma_layout(
        (),
        |_, slot| Ok::<u64, ()>(nonidentity_iova(slot)),
        |(), layout| {
            let mut programmed = Vec::new();
            for_each_initial_dma_program(&layout, |program| programmed.push(program));
            (
                programmed,
                (0..TX_SLOTS)
                    .map(|slot| layout.tx_descriptor_iova(slot))
                    .collect::<Vec<_>>(),
            )
        },
    )
    .expect("all DMA slots should authorize");

    assert_eq!(
        outputs.0[0],
        InitialDmaProgram::TxRingBase(nonidentity_iova(DmaSlot::TxRing))
    );
    for slot in 0..RX_SLOTS {
        assert!(outputs.0.contains(&InitialDmaProgram::RxDescriptor {
            slot,
            iova: nonidentity_iova(DmaSlot::RxBuffer(slot)),
        }));
    }
    assert!(outputs
        .0
        .contains(&InitialDmaProgram::RxRingBase(nonidentity_iova(
            DmaSlot::RxRing
        ))));
    assert_eq!(outputs.0.last(), Some(&InitialDmaProgram::Enable));
    assert_eq!(outputs.0.len(), RX_SLOTS + 3);
    for slot in 0..TX_SLOTS {
        assert_eq!(outputs.1[slot], nonidentity_iova(DmaSlot::TxBuffer(slot)));
    }
}

#[test]
fn authorization_rejection_skips_all_programming_and_enablement() {
    let mut writes = Vec::new();
    let result = with_authorized_dma_layout(
        (),
        |_, slot| {
            if slot == DmaSlot::TxBuffer(4) {
                Err("denied")
            } else {
                Ok(nonidentity_iova(slot))
            }
        },
        |(), layout| {
            for_each_initial_dma_program(&layout, |program| writes.push(program));
        },
    );

    assert!(matches!(result, Err("denied")));
    assert!(
        writes.is_empty(),
        "authorization denial must suppress DMA-address and TX/RX-enable programming"
    );
}

#[test]
fn controller_cannot_fall_back_to_cpu_physical_addresses() {
    let source = include_str!("../../../cells/drivers/e1000/src/controller.rs");

    assert!(
        !source.contains(".phys()"),
        "controller must never substitute CPU physical addresses for authorized IOVAs"
    );
    for accessor in [
        "for_each_initial_dma_program(&layout",
        "tx_descriptor_iova(slot)",
        "rx_descriptor_iova(head)",
    ] {
        assert!(
            source.contains(accessor),
            "controller must use authorized DMA accessor {accessor}"
        );
    }
    let boundary = source
        .find("        with_authorized_dma_layout(")
        .expect("controller initialization must use the authorization boundary");
    assert!(boundary < source.find("ctrl.wr32(TCTL").unwrap());
    assert!(boundary < source.find("ctrl.wr32(RCTL").unwrap());
}
