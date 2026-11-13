extern crate std;

use wire::display::{FramePlacement, Rect, SnapZone, ViewOpResult};

use super::placement::calculate_snap_rect;
use super::tests::dummy_req;
use super::*;

#[test]
fn test_six_zone_snap_geometry_and_min_size_refusal() {
    let mut reg = FrameRegistry::new();
    let tid = 100;
    let r1 = reg.create_view(&dummy_req(0, 0), tid, 10).unwrap();

    let work_area = Rect {
        x: 0,
        y: 0,
        w: 1280,
        h: 800 - 36,
    }; // 1280x764

    // Test all 6 zones
    let zones = [
        SnapZone::LeftHalf,
        SnapZone::RightHalf,
        SnapZone::TopLeftQuarter,
        SnapZone::BottomLeftQuarter,
        SnapZone::TopRightQuarter,
        SnapZone::BottomRightQuarter,
    ];

    // Another client cannot snap the frame
    let cur_gen = reg.frames.get(&r1.frame_id).unwrap().generation;
    assert_eq!(
        reg.snap_frame(r1.frame_id, cur_gen, SnapZone::LeftHalf, work_area, tid + 1),
        Err(ViewOpResult::AccessDenied)
    );

    for zone in zones {
        let cur_gen = reg.frames.get(&r1.frame_id).unwrap().generation;
        let (resp, rect) = reg
            .snap_frame(r1.frame_id, cur_gen, zone, work_area, tid)
            .expect("snap zone");
        assert_eq!(resp.result, ViewOpResult::Success);
        assert_eq!(rect, calculate_snap_rect(zone, work_area));
        let frame = reg.frames.get(&r1.frame_id).unwrap();
        assert_eq!(frame.placement, FramePlacement::Snapped(zone));
        assert_eq!(frame.w, rect.w);
        assert_eq!(frame.h, rect.h);
    }

    // Test min-size refusal: create frame with huge min_w = 1000
    let mut big_req = dummy_req(0, 0);
    big_req.w = 1200;
    big_req.min_w = 1000;
    let r2 = reg.create_view(&big_req, tid, 11).unwrap();
    let cur_gen2 = reg.frames.get(&r2.frame_id).unwrap().generation;

    // Snap to LeftHalf (width 640 < min_w 1000) must be Refused
    let refused = reg.snap_frame(r2.frame_id, cur_gen2, SnapZone::LeftHalf, work_area, tid);
    assert_eq!(refused, Err(ViewOpResult::Refused));
    // Frame remains floating
    assert_eq!(
        reg.frames.get(&r2.frame_id).unwrap().placement,
        FramePlacement::Floating
    );
}

#[test]
fn test_f11_fullscreen_toggle_and_state_restoration() {
    let mut reg = FrameRegistry::new();
    let tid = 100;
    let r1 = reg.create_view(&dummy_req(0, 0), tid, 10).unwrap();

    let cur_gen = reg.frames.get(&r1.frame_id).unwrap().generation;

    // Another client cannot fullscreen the frame
    assert_eq!(
        reg.toggle_fullscreen(r1.frame_id, cur_gen, true, 1280, 800, tid + 1),
        Err(ViewOpResult::AccessDenied)
    );

    // Enter fullscreen
    let (fs_resp, fs_rect) = reg
        .toggle_fullscreen(r1.frame_id, cur_gen, true, 1280, 800, tid)
        .expect("enter fullscreen");
    assert_eq!(fs_resp.result, ViewOpResult::Success);
    assert_eq!(
        fs_rect,
        Rect {
            x: 0,
            y: 0,
            w: 1280,
            h: 800
        }
    );
    let frame_fs = reg.frames.get(&r1.frame_id).unwrap();
    assert_eq!(frame_fs.placement, FramePlacement::Fullscreen);

    // Exit fullscreen (restore floating)
    let cur_gen_fs = frame_fs.generation;
    let (rest_resp, rest_rect) = reg
        .toggle_fullscreen(r1.frame_id, cur_gen_fs, false, 1280, 800, tid)
        .expect("exit fullscreen");
    assert_eq!(rest_resp.result, ViewOpResult::Success);
    assert_eq!(
        rest_rect,
        Rect {
            x: 100,
            y: 100,
            w: 800,
            h: 600
        }
    );
    let frame_rest = reg.frames.get(&r1.frame_id).unwrap();
    assert_eq!(frame_rest.placement, FramePlacement::Floating);
    assert_eq!(frame_rest.w, 800);
    assert_eq!(frame_rest.h, 600);

    // Snap to LeftHalf, then enter and exit fullscreen — verify Snapped placement is restored!
    let work_area = Rect {
        x: 0,
        y: 0,
        w: 1280,
        h: 764,
    };
    let cur_gen_rest = frame_rest.generation;
    let (snap_resp, snap_rect) = reg
        .snap_frame(
            r1.frame_id,
            cur_gen_rest,
            SnapZone::LeftHalf,
            work_area,
            tid,
        )
        .unwrap();
    assert_eq!(snap_resp.result, ViewOpResult::Success);

    let snap_gen = reg.frames.get(&r1.frame_id).unwrap().generation;
    let _ = reg
        .toggle_fullscreen(r1.frame_id, snap_gen, true, 1280, 800, tid)
        .unwrap();
    assert_eq!(
        reg.frames.get(&r1.frame_id).unwrap().placement,
        FramePlacement::Fullscreen
    );

    let fs_gen = reg.frames.get(&r1.frame_id).unwrap().generation;
    let (_, exit_rect) = reg
        .toggle_fullscreen(r1.frame_id, fs_gen, false, 1280, 800, tid)
        .unwrap();
    assert_eq!(exit_rect, snap_rect);
    assert_eq!(
        reg.frames.get(&r1.frame_id).unwrap().placement,
        FramePlacement::Snapped(SnapZone::LeftHalf)
    );
}
