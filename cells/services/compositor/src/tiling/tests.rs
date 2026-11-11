use api::display::{compositor_ops, SurfaceRole};

use super::*;
use crate::window_decoration::ResizeEdge;

struct World {
    table: SurfaceTable,
    frames: FrameRegistry,
    z_order: ZOrder,
}

impl World {
    fn new() -> Self {
        Self {
            table: SurfaceTable::new(),
            frames: FrameRegistry::new(),
            z_order: ZOrder::new(),
        }
    }

    /// A window whose visible frame is `visible`.
    fn window(&mut self, owner: usize, visible: Rect) -> u64 {
        let c = window_decoration::content_within(visible);
        let cap = self
            .table
            .create(c.x, c.y, c.w, c.h, owner, SurfaceRole::Interactive)
            .unwrap();
        self.frames.adopt(cap, owner);
        self.z_order.push(cap);
        cap
    }

    fn frame(&self, cap: u64) -> u32 {
        self.frames.frame_of(cap).unwrap()
    }

    fn visible(&self, cap: u64) -> Rect {
        window_decoration::frame_rect(self.table.get(cap).unwrap().screen_rect())
    }

    fn asked(&self, cap: u64) -> Rect {
        window_decoration::frame_rect(self.table.get(cap).unwrap().intended_rect())
    }
}

fn r(x: i32, y: i32, w: u32, h: u32) -> Rect {
    Rect { x, y, w, h }
}

#[test]
fn a_frame_dropped_near_an_edge_snaps_flush_to_it() {
    let mut world = World::new();
    let a = world.window(7, r(100, 100, 300, 200));
    let b = world.window(8, r(700, 700, 300, 200));
    let content = window_decoration::content_within(r(410, 120, 300, 200));
    let glue =
        glue_at(&world.frames, &world.table, &world.z_order, b, content).expect("10px right of a");
    assert_eq!(
        (glue.target, glue.side, glue.dropped_first),
        (a, Side::Right, false)
    );
    assert_eq!(
        window_decoration::frame_rect(glue.content),
        r(400, 120, 300, 200)
    );

    let below = window_decoration::content_within(r(120, 290, 300, 200));
    let glue = glue_at(&world.frames, &world.table, &world.z_order, b, below)
        .expect("overlapping a's bottom");
    assert_eq!((glue.side, glue.dropped_first), (Side::Below, false));
    assert_eq!(window_decoration::frame_rect(glue.content).y, 300);

    // Too far, or beside it but mostly past its end: no glue.
    let far = window_decoration::content_within(r(430, 100, 300, 200));
    assert!(glue_at(&world.frames, &world.table, &world.z_order, b, far).is_none());
    let past = window_decoration::content_within(r(400, 260, 300, 200));
    assert!(glue_at(&world.frames, &world.table, &world.z_order, b, past).is_none());
}

#[test]
fn a_tile_group_moves_and_rises_as_one() {
    let mut world = World::new();
    let a = world.window(7, r(100, 100, 300, 200));
    let b = world.window(8, r(400, 100, 300, 200));
    let other = world.window(9, r(0, 0, 50, 50));
    assert!(world
        .frames
        .glue(world.frame(a), world.frame(b), Side::Right));
    let surface = world.table.get_mut(a).unwrap();
    surface.move_to(surface.x + 30, surface.y + 13);
    follow(&world.frames, &mut world.table, a, (30, 13));
    assert_eq!(world.visible(b), r(430, 113, 300, 200));

    raise(&world.frames, &mut world.z_order, b);
    let order: Vec<u64> = world.z_order.iter_bottom_to_top().collect();
    assert_eq!(
        order,
        [other, a, b],
        "the group above the rest, the activated window on top"
    );
}

#[test]
fn dragging_a_shared_edge_resizes_both_sides_within_their_minimums() {
    let mut world = World::new();
    let a = world.window(7, r(100, 100, 300, 200));
    let b = world.window(8, r(400, 100, 300, 200));
    let c = world.window(9, r(400, 300, 300, 200));
    let (fa, fb, fc) = (world.frame(a), world.frame(b), world.frame(c));
    assert!(world.frames.glue(fa, fb, Side::Right) && world.frames.glue(fb, fc, Side::Below));
    let start = world.table.get(a).unwrap().screen_rect();
    assert!(resize(
        &world.frames,
        &mut world.table,
        a,
        start,
        ResizeEdge::East,
        (50, 0)
    ));
    assert_eq!(world.asked(a), r(100, 100, 350, 200));
    assert_eq!(
        world.asked(b),
        r(450, 100, 250, 200),
        "the neighbour gives up the width"
    );
    // c's left edge sits on the same line (b's column): it moves with it.
    assert_eq!(world.asked(c), r(450, 300, 250, 200));

    // Far past b's minimum: the line stops where b keeps 64px of content.
    let mut world2 = World::new();
    let a = world2.window(7, r(100, 100, 300, 200));
    let b = world2.window(8, r(400, 100, 300, 200));
    assert!(world2
        .frames
        .glue(world2.frame(a), world2.frame(b), Side::Right));
    let start = world2.table.get(a).unwrap().screen_rect();
    resize(
        &world2.frames,
        &mut world2.table,
        a,
        start,
        ResizeEdge::East,
        (1000, 0),
    );
    assert_eq!(world2.table.get(b).unwrap().intended_rect().w, 64);

    // The outer edge of a lone side is a plain resize.
    let start = world2.table.get(b).unwrap().screen_rect();
    let mut world3 = World::new();
    let lone = world3.window(7, r(100, 100, 300, 200));
    assert!(!resize(
        &world3.frames,
        &mut world3.table,
        lone,
        start,
        ResizeEdge::East,
        (10, 0)
    ));
}

#[test]
fn neighbours_catch_up_with_a_committed_edge() {
    let mut world = World::new();
    let a = world.window(7, r(100, 100, 300, 200));
    let b = world.window(8, r(400, 100, 300, 200));
    assert!(world
        .frames
        .glue(world.frame(a), world.frame(b), Side::Right));
    // a grew on its own (an owner-driven resize): b is asked to meet it.
    world.table.get_mut(a).unwrap().w += 20;
    settle(&world.frames, &mut world.table, a);
    assert_eq!(world.asked(b), r(420, 100, 280, 200));
}

#[test]
fn a_frame_leaving_the_middle_splits_the_group() {
    let mut world = World::new();
    let a = world.window(7, r(0, 100, 300, 200));
    let b = world.window(8, r(300, 100, 300, 200));
    let c = world.window(9, r(600, 100, 300, 200));
    let (fa, fb, fc) = (world.frame(a), world.frame(b), world.frame(c));
    assert!(world.frames.glue(fa, fb, Side::Right) && world.frames.glue(fb, fc, Side::Right));
    assert_eq!(group_windows(&world.frames, a).len(), 3);
    world.frames.remove_surface(b);
    world.table.remove(b);
    assert_eq!(group_windows(&world.frames, a), [a]);
    assert_eq!(group_windows(&world.frames, c), [c]);
}

#[test]
fn a_group_comes_back_from_minimized_together() {
    let mut world = World::new();
    let a = world.window(7, r(0, 100, 300, 200));
    let b = world.window(8, r(300, 100, 300, 200));
    assert!(world
        .frames
        .glue(world.frame(a), world.frame(b), Side::Right));
    // The user minimizes the group from the window menu.
    let (fa, fb) = (world.frame(a), world.frame(b));
    world.frames.mark_group_minimized(&[fa, fb]);
    for cap in [a, b] {
        let rect = world.table.get(cap).unwrap().screen_rect();
        world
            .table
            .get_mut(cap)
            .unwrap()
            .request_state(cap as u32, compositor_ops::MINIMIZE, rect);
    }
    let rect = world.table.get(a).unwrap().screen_rect();
    world
        .table
        .get_mut(a)
        .unwrap()
        .request_state(a as u32, compositor_ops::RESTORE, rect);
    assert!(restore_group(&mut world.frames, &mut world.table, a).is_some());
    assert_eq!(world.table.get(b).unwrap().state(), WindowState::Normal);
}

#[test]
fn a_window_its_own_app_minimized_stays_down_when_its_group_returns() {
    let mut world = World::new();
    let a = world.window(7, r(0, 100, 300, 200));
    let b = world.window(8, r(300, 100, 300, 200));
    assert!(world
        .frames
        .glue(world.frame(a), world.frame(b), Side::Right));
    for cap in [a, b] {
        let rect = world.table.get(cap).unwrap().screen_rect();
        world
            .table
            .get_mut(cap)
            .unwrap()
            .request_state(cap as u32, compositor_ops::MINIMIZE, rect);
    }
    let rect = world.table.get(a).unwrap().screen_rect();
    world
        .table
        .get_mut(a)
        .unwrap()
        .request_state(a as u32, compositor_ops::RESTORE, rect);
    assert!(restore_group(&mut world.frames, &mut world.table, a).is_none());
    assert_eq!(world.table.get(b).unwrap().state(), WindowState::Minimized);
}

#[test]
fn a_tile_group_holds_at_most_sixteen_frames() {
    let mut world = World::new();
    let caps: Vec<u64> = (0..17)
        .map(|i| world.window(7, r(i * 60, 0, 60, 100)))
        .collect();
    for pair in caps.windows(2).take(15) {
        assert!(world
            .frames
            .glue(world.frame(pair[0]), world.frame(pair[1]), Side::Right));
    }
    assert!(!world
        .frames
        .glue(world.frame(caps[15]), world.frame(caps[16]), Side::Right));
}

#[test]
fn a_window_far_off_screen_never_glues_and_nothing_overflows() {
    let mut world = World::new();
    let far = world
        .table
        .create(
            i32::MIN + 40,
            i32::MAX - 300,
            300,
            200,
            9,
            SurfaceRole::Interactive,
        )
        .unwrap();
    world.frames.adopt(far, 9);
    world.z_order.push(far);
    let me = world.window(7, r(100, 100, 300, 200));
    let content = world.table.get(me).unwrap().screen_rect();
    assert!(glue_at(&world.frames, &world.table, &world.z_order, me, content).is_none());
}
