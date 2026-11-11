use api::display::{ConfigureKind, SurfaceRole, WindowState};

use super::reconcile::reconcile;
use super::*;

const MAXIMUM: Rect = Rect {
    x: 1,
    y: 28,
    w: 1278,
    h: 735,
};

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

    fn window(&mut self, owner: usize, rect: Rect) -> u64 {
        let cap = self
            .table
            .create(
                rect.x,
                rect.y,
                rect.w,
                rect.h,
                owner,
                SurfaceRole::Interactive,
            )
            .unwrap();
        self.z_order.push(cap);
        self.frames.adopt(cap, owner);
        cap
    }

    /// The user stacks `source`'s frame onto `target`'s.
    fn stack(&mut self, source: u64, target: u64) {
        stack(
            &mut self.frames,
            &mut self.table,
            &mut self.z_order,
            source,
            target,
        )
        .expect("stacked");
    }

    /// Put `source`'s frame into `target`'s with no proposals: a stack
    /// whose windows are already where the test says.
    fn link(&mut self, source: u64, target: u64) {
        let (from, onto) = (
            self.frames.frame_of(source).unwrap(),
            self.frames.frame_of(target).unwrap(),
        );
        assert!(self.frames.stack(from, onto));
        sync(&self.frames, &mut self.table, onto);
    }

    fn origin(&self, cap: u64) -> (i32, i32) {
        let surface = self.table.get(cap).unwrap();
        (surface.x, surface.y)
    }

    fn z(&self) -> Vec<u64> {
        self.z_order.iter_bottom_to_top().collect()
    }
}

fn rect(x: i32, y: i32, w: u32, h: u32) -> Rect {
    Rect { x, y, w, h }
}

#[test]
fn a_stack_shows_one_window_at_its_place_and_hides_the_rest_from_input() {
    let mut world = World::new();
    let a = world.window(7, rect(100, 80, 300, 200));
    let b = world.window(8, rect(500, 400, 300, 200));
    world.stack(b, a);

    assert_eq!(
        world.origin(b),
        (100, 80),
        "the dropped window takes the target's place"
    );
    assert_eq!(world.origin(a), (100, 80));
    let (shown, hidden) = (world.table.get(b).unwrap(), world.table.get(a).unwrap());
    assert!(shown.is_shown() && shown.accepts_input());
    assert!(
        !hidden.is_shown() && !hidden.accepts_input(),
        "a hidden tab takes no input"
    );
    assert_eq!(
        hidden.state(),
        WindowState::Normal,
        "a hidden tab stays configurable"
    );
    assert_eq!(tabs_of(&world.frames, b), Tabs::stack(2, 1));
}

#[test]
fn picking_a_tab_swaps_places_and_asks_it_for_the_frame_size() {
    let mut world = World::new();
    let a = world.window(7, rect(100, 80, 300, 200));
    let other = world.window(9, rect(0, 0, 50, 50));
    let b = world.window(8, rect(500, 400, 120, 90));
    world.stack(b, a);
    assert_eq!(
        world.z(),
        [b, other, a],
        "the dropped window takes the target's place"
    );
    assert_eq!(
        world.table.get(b).unwrap().intended_rect(),
        rect(100, 80, 300, 200)
    );

    select(&mut world.frames, &mut world.table, &mut world.z_order, a).expect("a was hidden");
    assert_eq!(
        world.z(),
        [a, other, b],
        "the picked tab takes the shown tab's place"
    );
    assert!(world.table.get(a).unwrap().is_shown());
    assert!(!world.table.get(b).unwrap().is_shown());
    assert!(
        !world.table.get(a).unwrap().is_configuring(),
        "a already has the frame's size"
    );
    assert!(
        select(&mut world.frames, &mut world.table, &mut world.z_order, a).is_none(),
        "already shown"
    );
}

#[test]
fn hidden_tabs_follow_the_shown_size_one_request_at_a_time() {
    let mut world = World::new();
    let a = world.window(7, rect(100, 80, 300, 200));
    let b = world.window(8, rect(100, 80, 120, 90));
    let c = world.window(9, rect(100, 80, 120, 90));
    world.link(b, a);
    world.link(c, a);
    world.frames.select(a);
    let frame = world.frames.frame_of(a).unwrap();
    sync(&world.frames, &mut world.table, frame);
    // b never answered an earlier proposal; c must not wait for it.
    let silent = rect(100, 80, 10, 10);
    world
        .table
        .get_mut(b)
        .unwrap()
        .begin_configure(b as u32, ConfigureKind::Resize, silent)
        .unwrap();
    reconcile(&world.frames, &mut world.table, frame, MAXIMUM);
    assert_eq!(
        world.table.get(b).unwrap().intended_rect(),
        silent,
        "b keeps its one request"
    );
    assert_eq!(
        world.table.get(c).unwrap().intended_rect(),
        rect(100, 80, 300, 200)
    );
    assert!(
        !world.table.get(a).unwrap().is_configuring(),
        "the shown tab is never asked"
    );
}

#[test]
fn the_frame_waits_while_its_shown_tab_is_being_resized() {
    let mut world = World::new();
    let a = world.window(7, rect(100, 80, 300, 200));
    let b = world.window(8, rect(100, 80, 120, 90));
    world.link(b, a);
    // b is shown and asked for a new size by a user resize.
    world
        .table
        .get_mut(b)
        .unwrap()
        .begin_configure(b as u32, ConfigureKind::Resize, rect(100, 80, 400, 300))
        .unwrap();
    let frame = world.frames.frame_of(a).unwrap();
    reconcile(&world.frames, &mut world.table, frame, MAXIMUM);
    assert!(
        !world.table.get(a).unwrap().is_configuring(),
        "a waits for b's final size"
    );
}

#[test]
fn proposals_never_exceed_the_work_area() {
    let mut world = World::new();
    let huge = world.window(7, rect(0, 0, 8000, 8000));
    let small = world.window(8, rect(0, 0, 100, 100));
    world.link(huge, small);
    let frame = world.frames.frame_of(small).unwrap();
    reconcile(&world.frames, &mut world.table, frame, MAXIMUM);
    let asked = world.table.get(small).unwrap().intended_rect();
    assert_eq!((asked.w, asked.h), (MAXIMUM.w, MAXIMUM.h));
}

#[test]
fn a_destroyed_shown_tab_leaves_its_neighbour_in_its_place() {
    let mut world = World::new();
    let a = world.window(7, rect(100, 80, 300, 200));
    let other = world.window(9, rect(0, 0, 50, 50));
    let b = world.window(8, rect(100, 80, 300, 200));
    let c = world.window(8, rect(100, 80, 300, 200));
    world.link(b, a);
    world.link(c, a);
    select(&mut world.frames, &mut world.table, &mut world.z_order, b);
    world.z_order.raise(b);
    // Tabs [a, b, c], b shown at the top.
    forget(&mut world.frames, &mut world.table, &mut world.z_order, b);
    world.z_order.remove(b);
    world.table.remove(b);
    assert_eq!(
        world.frames.selected_cap(world.frames.frame_of(a).unwrap()),
        Some(c)
    );
    assert!(world.table.get(c).unwrap().is_shown());
    assert_eq!(world.z().last(), Some(&c), "c took b's place on top");
    world.frames.assert_consistent(&world.table);

    // A hidden tab leaves without changing what is shown.
    assert!(forget(&mut world.frames, &mut world.table, &mut world.z_order, a).is_none());
    world.z_order.remove(a);
    world.table.remove(a);
    assert_eq!(tabs_of(&world.frames, c), Tabs::SINGLE);
    let _ = other;
}

#[test]
fn closing_the_shown_tab_shows_the_next_live_tab() {
    let mut world = World::new();
    let a = world.window(7, rect(100, 80, 300, 200));
    let b = world.window(8, rect(100, 80, 300, 200));
    let c = world.window(9, rect(100, 80, 300, 200));
    world.link(b, a);
    world.link(c, a);
    select(&mut world.frames, &mut world.table, &mut world.z_order, b);
    // c (after b) is already closing: the live tab before b is shown.
    world
        .table
        .get_mut(c)
        .unwrap()
        .begin_close(c as u32)
        .unwrap();
    world
        .table
        .get_mut(b)
        .unwrap()
        .begin_close(b as u32)
        .unwrap();
    let (shown, _) = show_next(&mut world.frames, &mut world.table, &mut world.z_order, b).unwrap();
    assert_eq!(shown, a);
    assert!(world.table.get(a).unwrap().is_shown());
    // A rejected close leaves b as a hidden tab.
    assert!(show_next(&mut world.frames, &mut world.table, &mut world.z_order, b).is_none());
}

#[test]
fn moving_one_tab_moves_the_whole_stack() {
    let mut world = World::new();
    let a = world.window(7, rect(100, 80, 300, 200));
    let b = world.window(8, rect(100, 80, 300, 200));
    world.link(b, a);
    // The owner of the hidden tab moves it: the frame goes with it.
    world.table.get_mut(a).unwrap().move_to(40, 50);
    follow(&world.frames, &mut world.table, a);
    assert_eq!(world.origin(b), (40, 50));
    assert_eq!(world.origin(a), (40, 50));
}

#[test]
fn detaching_puts_the_tab_beside_its_stack_and_shows_the_next_tab() {
    let mut world = World::new();
    let a = world.window(7, rect(100, 80, 300, 200));
    let b = world.window(8, rect(100, 80, 300, 200));
    world.link(b, a);
    assert!(world.table.get(b).unwrap().is_shown());
    detach(
        &mut world.frames,
        &mut world.table,
        &mut world.z_order,
        b,
        (1280, 800),
        None,
    )
    .unwrap();
    assert_eq!(world.origin(b), (124, 104));
    assert!(world.table.get(a).unwrap().is_shown() && world.table.get(b).unwrap().is_shown());
    assert_ne!(world.frames.frame_of(a), world.frames.frame_of(b));
    // Near the screen's edge the tab stays reachable.
    world.link(b, a);
    world.table.get_mut(a).unwrap().move_to(1250, 790);
    follow(&world.frames, &mut world.table, a);
    detach(
        &mut world.frames,
        &mut world.table,
        &mut world.z_order,
        b,
        (1280, 800),
        None,
    )
    .unwrap();
    assert_eq!(world.origin(b), (1280 - 96, 800 - 96));
}

#[test]
fn close_all_asks_one_tab_at_a_time_in_tab_order() {
    let mut world = World::new();
    let a = world.window(7, rect(100, 80, 300, 200));
    let b = world.window(8, rect(100, 80, 300, 200));
    let c = world.window(9, rect(100, 80, 300, 200));
    world.link(b, a);
    world.link(c, a);
    let frame = world.frames.frame_of(a).unwrap();
    let closing =
        |world: &World, cap| world.table.get(cap).unwrap().state() == WindowState::Closing;

    close_all(&mut world.frames, &mut world.table, &mut world.z_order, c).unwrap();
    assert!(
        closing(&world, a) && !closing(&world, b) && !closing(&world, c),
        "a is asked first"
    );
    assert_eq!(
        world.frames.selected_cap(frame),
        Some(b),
        "the next live tab shows meanwhile"
    );

    // a refuses: b is asked next.
    let answer = |world: &mut World, cap: u64, action| {
        let surface = world.table.get_mut(cap).unwrap();
        let serial = surface.close_serial().unwrap();
        surface.close_response(cap as u32, serial, action);
    };
    answer(&mut world, a, api::display::CloseResponseAction::Reject);
    advance_close(
        &mut world.frames,
        &mut world.table,
        &mut world.z_order,
        frame,
    )
    .unwrap();
    assert!(closing(&world, b) && !closing(&world, a));
    // b accepts but is not gone yet: c is asked, and nothing is left after it.
    answer(&mut world, b, api::display::CloseResponseAction::Accept);
    advance_close(
        &mut world.frames,
        &mut world.table,
        &mut world.z_order,
        frame,
    )
    .unwrap();
    assert!(closing(&world, c));
    assert!(advance_close(
        &mut world.frames,
        &mut world.table,
        &mut world.z_order,
        frame
    )
    .is_none());
    assert!(!world.frames.is_closing_all(frame));
}

#[test]
fn close_all_moves_on_when_the_asked_tab_goes_away_unanswered() {
    let mut world = World::new();
    let a = world.window(7, rect(100, 80, 300, 200));
    let b = world.window(8, rect(100, 80, 300, 200));
    world.link(b, a);
    let frame = world.frames.frame_of(a).unwrap();
    close_all(&mut world.frames, &mut world.table, &mut world.z_order, a).unwrap();
    assert!(world.table.get(a).unwrap().awaits_close_answer());
    // a's owner destroys it without answering: b is asked next.
    forget(&mut world.frames, &mut world.table, &mut world.z_order, a);
    world.z_order.remove(a);
    world.table.remove(a);
    assert!(world.table.get(b).unwrap().awaits_close_answer());
    assert!(world.frames.is_closing_all(frame), "b's answer ends it");
}
