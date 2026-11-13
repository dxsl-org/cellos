extern crate std;

use wire::display::{Rect, SnapZone, ViewOpResult};

use super::tests::dummy_req;
use super::*;

const A: usize = 100;
const B: usize = 200;
const WORK: Rect = Rect {
    x: 0,
    y: 0,
    w: 1280,
    h: 764,
};

fn view_of(reg: &FrameRegistry, cap: u64) -> (u32, u32) {
    let id = reg.by_cap[&cap];
    (id, reg.views[&id].incarnation)
}

fn generation(reg: &FrameRegistry, frame: u32) -> u32 {
    reg.frames[&frame].generation
}

/// A's window 10 with B's window 20 stacked onto it.
fn mixed_stack() -> (FrameRegistry, u32) {
    let mut reg = FrameRegistry::new();
    let a = reg.adopt(10, A).unwrap();
    let b = reg.adopt(20, B).unwrap();
    assert_eq!(reg.frames[&a].owner_tid, Some(A));
    assert!(reg.stack(b, a));
    (reg, a)
}

#[test]
fn stacking_two_apps_makes_the_frame_the_compositors() {
    let (reg, frame) = mixed_stack();
    assert_eq!(reg.members(frame), [10, 20]);
    assert_eq!(
        reg.selected_cap(frame),
        Some(20),
        "the dropped window is shown"
    );
    assert!(reg.is_hidden(10) && !reg.is_hidden(20));
    assert_eq!(reg.frames[&frame].owner_tid, None);
    assert_eq!(
        reg.frames.len(),
        1,
        "the dropped window's own frame is gone"
    );
}

#[test]
fn no_client_may_drive_a_frame_shared_with_another_app() {
    let (mut reg, frame) = mixed_stack();
    let gen = generation(&reg, frame);
    let (a_view, a_inc) = view_of(&reg, 10);
    let (b_view, b_inc) = view_of(&reg, 20);
    for (tid, view, inc) in [(A, a_view, a_inc), (A, b_view, b_inc), (B, b_view, b_inc)] {
        assert_eq!(
            reg.select_view(frame, gen, view, inc, tid),
            Err(ViewOpResult::AccessDenied)
        );
    }
    for tid in [A, B] {
        assert_eq!(
            reg.snap_frame(frame, gen, SnapZone::LeftHalf, WORK, tid),
            Err(ViewOpResult::AccessDenied)
        );
        assert_eq!(
            reg.toggle_fullscreen(frame, gen, true, 1280, 800, tid),
            Err(ViewOpResult::AccessDenied)
        );
        assert_eq!(
            reg.create_view(&dummy_req(frame, gen), tid, 30),
            Err(ViewOpResult::AccessDenied)
        );
    }
    assert_eq!(reg.selected_cap(frame), Some(20), "nothing changed");
}

#[test]
fn detaching_gives_each_app_its_own_frame_back() {
    let (mut reg, frame) = mixed_stack();
    let (new_frame, change) = reg.detach(20).unwrap();
    assert_eq!(
        change,
        Reselected {
            frame_id: frame,
            selected: Some(10)
        }
    );
    assert_eq!(reg.frames[&frame].owner_tid, Some(A));
    assert_eq!(reg.frames[&new_frame].owner_tid, Some(B));
    assert_eq!(reg.members(new_frame), [20]);
    assert!(
        reg.detach(10).is_none(),
        "a window alone in its frame stays"
    );
}

#[test]
fn removing_the_shown_tab_shows_its_neighbour_and_the_last_drops_the_frame() {
    let mut reg = FrameRegistry::new();
    let frame = reg.adopt(10, A).unwrap();
    for cap in [11, 12] {
        let other = reg.adopt(cap, A).unwrap();
        assert!(reg.stack(other, frame));
    }
    reg.select(11);
    assert_eq!(
        reg.remove_surface(11),
        Some(Reselected {
            frame_id: frame,
            selected: Some(12)
        })
    );
    assert_eq!(
        reg.remove_surface(12),
        Some(Reselected {
            frame_id: frame,
            selected: Some(10)
        })
    );
    assert_eq!(
        reg.remove_surface(10),
        Some(Reselected {
            frame_id: frame,
            selected: None
        })
    );
    assert!(reg.frames.is_empty() && reg.views.is_empty() && reg.by_cap.is_empty());
}

#[test]
fn an_apps_death_hands_a_shared_frame_to_the_survivor() {
    let mut reg = FrameRegistry::new();
    let mut req = dummy_req(0, 0);
    req.min_w = 700;
    let a = reg.create_view(&req, A, 10).unwrap().frame_id;
    let b = reg.adopt(20, B).unwrap();
    assert!(reg.stack(b, a));
    assert_eq!(reg.frames[&a].min_w, 700);
    reg.select(10);
    let changed = reg.cleanup_owner(A);
    assert_eq!(
        changed,
        [Reselected {
            frame_id: a,
            selected: Some(20)
        }]
    );
    assert_eq!(reg.frames[&a].owner_tid, Some(B), "never the dead tid");
    assert_eq!(
        reg.frames[&a].min_w, 0,
        "the dead view's minimum went with it"
    );
}

#[test]
fn a_minimum_larger_than_the_window_is_refused() {
    let mut reg = FrameRegistry::new();
    let mut req = dummy_req(0, 0);
    req.min_h = req.h + 1;
    assert_eq!(reg.create_view(&req, A, 10), Err(ViewOpResult::Refused));
    assert!(reg.frames.is_empty());
}
