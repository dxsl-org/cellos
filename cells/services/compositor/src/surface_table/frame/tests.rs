extern crate std;

use api::display::{CreateViewRequest, FrameRef, SurfaceRole, ViewOpResult, ViewRef};

use super::*;

pub(super) fn dummy_req(frame_id: u32, frame_gen: u32) -> CreateViewRequest {
    CreateViewRequest {
        opcode: api::display::compositor_ops::CREATE_VIEW,
        role: SurfaceRole::Interactive,
        _pad: [0; 2],
        frame_id,
        frame_generation: frame_gen,
        w: 800,
        h: 600,
        min_w: 400,
        min_h: 300,
        recipient_incarnation: 1,
        file_grant_id: 0,
        request_generation: 1,
        _reserved: 0,
        launch_token: [0; 16],
    }
}

pub(super) fn frame_ref(frame_id: u32, generation: u32) -> FrameRef {
    FrameRef {
        frame_id,
        generation,
    }
}

pub(super) fn view_ref(view_id: u32, incarnation: u32) -> ViewRef {
    ViewRef {
        view_id,
        incarnation,
    }
}

#[test]
fn test_group_view_same_frame_does_not_panic() {
    let mut reg = FrameRegistry::new();
    let tid_a = 100;
    let r1 = reg.create_view(&dummy_req(0, 0), tid_a, 10).unwrap();
    // Call group_view with target_frame_id == source_frame_id
    let res = reg.group_view(
        frame_ref(r1.frame_id, r1.frame_generation),
        frame_ref(r1.frame_id, r1.frame_generation),
        view_ref(r1.view_id, r1.view_incarnation),
        tid_a,
    );
    assert!(
        res.is_ok(),
        "same-frame group_view must succeed or no-op safely without panicking"
    );
}

#[test]
fn test_multi_view_creation_and_frame_association() {
    let mut reg = FrameRegistry::new();
    let tid_a = 100;
    let tid_b = 200;

    // Process A creates initial view in a new frame
    let req1 = dummy_req(0, 0);
    let resp1 = reg.create_view(&req1, tid_a, 10).expect("create view 1");
    assert_eq!(resp1.result, ViewOpResult::Success);
    assert_eq!(resp1.frame_id, 1);
    assert_eq!(resp1.frame_generation, 1);
    assert_eq!(resp1.view_id, 1);

    // Process A creates a second view in the same frame
    let req2 = dummy_req(1, 1);
    let resp2 = reg.create_view(&req2, tid_a, 11).expect("create view 2");
    assert_eq!(resp2.frame_id, 1);
    assert_eq!(resp2.view_id, 2);

    // Process B cannot plant a view in A's frame
    let req3 = dummy_req(resp2.frame_id, resp2.frame_generation);
    assert_eq!(
        reg.create_view(&req3, tid_b, 12),
        Err(ViewOpResult::AccessDenied)
    );

    let frame = reg.frames.get(&1).expect("frame 1 exists");
    assert_eq!(frame.views.len(), 2);
    assert_eq!(frame.generation, resp2.frame_generation);
    assert_eq!(frame.views[0].view_id, 1);
    assert_eq!(frame.views[1].view_id, 2);
    assert_eq!(frame.selected_view.map(|v| v.view_id), Some(1));
    assert_eq!(reg.views.len(), 2);
}

#[test]
fn test_select_view_and_stale_generation_rejection() {
    let mut reg = FrameRegistry::new();
    let tid_a = 100;

    let resp1 = reg.create_view(&dummy_req(0, 0), tid_a, 10).unwrap();
    let resp2 = reg
        .create_view(
            &dummy_req(resp1.frame_id, resp1.frame_generation),
            tid_a,
            11,
        )
        .unwrap();

    // Another client cannot select a tab in A's frame
    assert_eq!(
        reg.select_view(
            resp2.frame_id,
            resp2.frame_generation,
            resp2.view_id,
            resp2.view_incarnation,
            200,
        ),
        Err(ViewOpResult::AccessDenied)
    );

    // Select view 2
    let sel = reg
        .select_view(
            resp2.frame_id,
            resp2.frame_generation,
            resp2.view_id,
            resp2.view_incarnation,
            tid_a,
        )
        .unwrap();
    assert_eq!(sel.result, ViewOpResult::Success);
    assert_eq!(
        reg.selected_view_for_frame(resp2.frame_id)
            .map(|v| v.view_id),
        Some(resp2.view_id)
    );

    // Stale frame generation rejected
    let err1 = reg.select_view(
        resp2.frame_id,
        999,
        resp2.view_id,
        resp2.view_incarnation,
        tid_a,
    );
    assert_eq!(err1, Err(ViewOpResult::StaleGeneration));

    // Stale view incarnation rejected
    let err2 = reg.select_view(
        resp2.frame_id,
        resp2.frame_generation,
        resp2.view_id,
        999,
        tid_a,
    );
    assert_eq!(err2, Err(ViewOpResult::StaleGeneration));

    // Nonexistent view rejected
    let err3 = reg.select_view(resp2.frame_id, resp2.frame_generation, 999, 1, tid_a);
    assert_eq!(err3, Err(ViewOpResult::ViewNotFound));
}

#[test]
fn test_group_and_detach_view() {
    let mut reg = FrameRegistry::new();
    let tid_a = 100;
    let tid_b = 200;

    let r1 = reg.create_view(&dummy_req(0, 0), tid_a, 10).unwrap();
    let r2 = reg
        .create_view(&dummy_req(r1.frame_id, r1.frame_generation), tid_a, 11)
        .unwrap();
    let r3 = reg
        .create_view(&dummy_req(r2.frame_id, r2.frame_generation), tid_a, 12)
        .unwrap();
    let rb = reg.create_view(&dummy_req(0, 0), tid_b, 13).unwrap();

    // A forged caller cannot detach view 3
    let forged_detach = reg.detach_view(
        frame_ref(r3.frame_id, r3.frame_generation),
        view_ref(r3.view_id, r3.view_incarnation),
        200,
        200,
        tid_b,
    );
    assert_eq!(forged_detach, Err(ViewOpResult::AccessDenied));

    // Detach view 3 into new floating frame at (200, 200)
    let detach = reg
        .detach_view(
            frame_ref(r3.frame_id, r3.frame_generation),
            view_ref(r3.view_id, r3.view_incarnation),
            200,
            200,
            tid_a,
        )
        .unwrap();
    assert_eq!(detach.result, ViewOpResult::Success);
    let new_frame_id = detach.frame_id;
    assert_ne!(new_frame_id, r1.frame_id);
    assert_eq!(reg.frame_for_view(r3.view_id), Some(new_frame_id));
    assert_eq!(reg.frames.get(&r1.frame_id).unwrap().views.len(), 2);
    assert_eq!(
        reg.frames.get(&r1.frame_id).unwrap().generation,
        r3.frame_generation + 1
    );
    let cur_r1_gen = reg.frames.get(&r1.frame_id).unwrap().generation;

    // B cannot group its own view into A's frame
    let foreign_target = reg.group_view(
        frame_ref(r1.frame_id, cur_r1_gen),
        frame_ref(rb.frame_id, rb.frame_generation),
        view_ref(rb.view_id, rb.view_incarnation),
        tid_b,
    );
    assert_eq!(foreign_target, Err(ViewOpResult::AccessDenied));
    // B cannot move A's view either
    let forged = reg.group_view(
        frame_ref(rb.frame_id, rb.frame_generation),
        frame_ref(new_frame_id, detach.frame_generation),
        view_ref(r3.view_id, r3.view_incarnation),
        tid_b,
    );
    assert_eq!(forged, Err(ViewOpResult::AccessDenied));

    // The owner groups view 3 back into frame 1
    let group = reg
        .group_view(
            frame_ref(r1.frame_id, cur_r1_gen),
            frame_ref(new_frame_id, detach.frame_generation),
            view_ref(r3.view_id, r3.view_incarnation),
            tid_a,
        )
        .unwrap();
    assert_eq!(group.result, ViewOpResult::Success);
    assert_eq!(reg.frame_for_view(r3.view_id), Some(r1.frame_id));
    assert_eq!(reg.frames.get(&r1.frame_id).unwrap().views.len(), 3);
    // Empty source frame was destroyed
    assert!(!reg.frames.contains_key(&new_frame_id));
}

#[test]
fn test_owner_crash_cleanup_and_focus_preservation() {
    let mut reg = FrameRegistry::new();
    let tid_a = 100;
    let tid_b = 200;

    let r1 = reg.create_view(&dummy_req(0, 0), tid_a, 10).unwrap();
    let r2 = reg
        .create_view(&dummy_req(r1.frame_id, r1.frame_generation), tid_a, 11)
        .unwrap();
    let r3 = reg.create_view(&dummy_req(0, 0), tid_b, 12).unwrap();

    // Select view 2 in A's frame
    reg.select_view(
        r2.frame_id,
        r2.frame_generation,
        r2.view_id,
        r2.view_incarnation,
        tid_a,
    )
    .unwrap();

    // Process B crashes: its frame goes, A's frame and selection survive
    assert!(reg.cleanup_owner(tid_b).is_empty());
    assert_eq!(reg.views.len(), 2);
    assert!(!reg.frames.contains_key(&r3.frame_id));
    assert_eq!(reg.frames.get(&r1.frame_id).unwrap().views.len(), 2);
    assert_eq!(
        reg.selected_view_for_frame(r1.frame_id).map(|v| v.view_id),
        Some(r2.view_id)
    );

    // Destroy view 1
    let cur_gen1 = reg.frames.get(&r1.frame_id).unwrap().generation;
    let (d1, cap1, _) = reg
        .destroy_view(
            r1.frame_id,
            cur_gen1,
            r1.view_id,
            r1.view_incarnation,
            tid_a,
        )
        .unwrap();
    assert_eq!(cap1, 10);
    assert_eq!(d1.result, ViewOpResult::Success);
    assert_eq!(
        reg.selected_view_for_frame(r1.frame_id).map(|v| v.view_id),
        Some(r2.view_id)
    );

    // Destroy view 2 -> frame destroyed
    let cur_gen2 = reg.frames.get(&r1.frame_id).unwrap().generation;
    let (d2, cap2, _) = reg
        .destroy_view(
            r1.frame_id,
            cur_gen2,
            r2.view_id,
            r2.view_incarnation,
            tid_a,
        )
        .unwrap();
    assert_eq!(cap2, 11);
    assert_eq!(d2.result, ViewOpResult::Success);
    assert!(reg.frames.is_empty());
    assert!(reg.views.is_empty());
}
