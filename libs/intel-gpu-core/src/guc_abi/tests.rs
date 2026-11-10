use super::*;

#[test]
fn register_context_is_the_upstream_twelve_dwords() {
    let desc = 0x0000_0001_0090_4119u64;
    assert_eq!(
        register_context(5, GUC_RENDER_CLASS, 1, desc),
        [0x4502, 1, 5, 0, 1, 0, 0, 0, 0, 0, 0x0090_4119, 1]
    );
}

#[test]
fn scheduling_and_deregistration_actions() {
    assert_eq!(sched_context_mode_set(5, true), [0x1001, 5, 1]);
    assert_eq!(sched_context_mode_set(5, false), [0x1001, 5, 0]);
    assert_eq!(sched_context(5), [0x1000, 5]);
    assert_eq!(deregister_context(5), [0x4503, 5]);
    assert_eq!(tlb_invalidation_full(3), [0x7000, 3, 0x8000_0000]);
}

#[test]
fn mmio_requests_carry_the_klv_header() {
    assert_eq!(
        self_cfg(KLV_H2G_CTB_ADDR, 2, 0x1_0000_1000),
        [0x0508, 0x0902_0002, 0x1000, 1]
    );
    assert_eq!(
        self_cfg(KLV_G2H_CTB_SIZE, 1, 0x2_0000),
        [0x0508, 0x0907_0001, 0x2_0000, 0]
    );
    assert_eq!(control_ctb(true), [0x4509, 1]);
}

#[test]
fn mmio_replies_decode_by_origin_and_type() {
    assert_eq!(decode_mmio_reply(0x0508), MmioReply::Pending);
    assert_eq!(decode_mmio_reply(0xf000_0001), MmioReply::Success(1));
    assert_eq!(decode_mmio_reply(0xb000_0000), MmioReply::Busy);
    assert_eq!(decode_mmio_reply(0xd000_0000), MmioReply::Retry);
    assert_eq!(
        decode_mmio_reply(0xe012_0034),
        MmioReply::Failure {
            hint: 0x12,
            error: 0x34
        }
    );
    assert_eq!(decode_mmio_reply(0x9000_0000), MmioReply::Unexpected(1));
}

#[test]
fn g2h_events_decode_their_payloads() {
    let event = |action: u32, payload: &[u32]| {
        let mut hxg = alloc::vec![HXG_ORIGIN_GUC | 1 << 28 | action];
        hxg.extend_from_slice(payload);
        decode_g2h(&hxg)
    };
    assert_eq!(
        event(0x1002, &[7, 1]),
        G2hEvent::SchedDone {
            guc_id: 7,
            enabled: true
        }
    );
    assert_eq!(event(0x4600, &[7]), G2hEvent::DeregisterDone { guc_id: 7 });
    assert_eq!(event(0x1008, &[7]), G2hEvent::ContextReset { guc_id: 7 });
    assert_eq!(event(0x6000, &[7]), G2hEvent::CatError { guc_id: 7 });
    assert_eq!(
        event(0x7001, &[9]),
        G2hEvent::TlbInvalidationDone { seqno: 9 }
    );
    // Truncated or unknown events are reported, never guessed.
    assert_eq!(event(0x1008, &[]), G2hEvent::Other { action: 0x1008 });
    assert_eq!(event(0x8003, &[0]), G2hEvent::Other { action: 0x8003 });
    // A response, not an event.
    assert_eq!(
        decode_g2h(&[HXG_ORIGIN_GUC | 7 << 28 | 0x1008, 7]),
        G2hEvent::Other { action: 0x1008 }
    );
}
