use api::display::SurfaceRole;
use api::input::{KeyEvent, KeyState, KeySym, Modifiers, MouseButton};

use super::*;

struct Desk {
    table: SurfaceTable,
    frames: FrameRegistry,
    z_order: ZOrder,
    router: PointerRouter,
}

impl Desk {
    fn new() -> Self {
        Self {
            table: SurfaceTable::new(),
            frames: FrameRegistry::new(),
            z_order: ZOrder::new(),
            router: PointerRouter::new(),
        }
    }

    fn window(&mut self, owner: usize, x: i32, y: i32) -> u64 {
        let cap = self
            .table
            .create(x, y, 300, 200, owner, SurfaceRole::Interactive)
            .unwrap();
        self.frames.adopt(cap, owner);
        self.z_order.push(cap);
        cap
    }

    fn route(&mut self, event: InputEvent, x: i32, y: i32) {
        let maximum = window_decoration::maximized_content(1280, 800);
        let context = RouteContext::new(
            maximum,
            (1280, 800),
            0,
            &mut self.table,
            &mut self.frames,
            &mut self.z_order,
            |_| {},
        );
        self.router.route(event, x, y, context);
    }

    fn drag(&mut self, from: (i32, i32), to: (i32, i32)) {
        let button = |state| InputEvent::MouseButton {
            button: MouseButton::Left,
            state,
        };
        self.route(button(KeyState::Pressed), from.0, from.1);
        self.route(
            InputEvent::MouseMove {
                x: to.0,
                y: to.1,
                dx: 0,
                dy: 0,
            },
            to.0,
            to.1,
        );
        self.route(button(KeyState::Released), to.0, to.1);
    }

    fn hold(&mut self, scancode: u32, state: KeyState) {
        self.router.note_key(&KeyEvent {
            timestamp_ticks: 0,
            scancode,
            keysym: KeySym::Unknown,
            character: 0,
            modifiers: Modifiers(0),
            state,
            _pad: [0; 2],
        });
    }

    /// A point on the tab of the shown window `cap`, right of its icon.
    fn tab(&self, cap: u64) -> (i32, i32) {
        let rect = self.table.get(cap).unwrap().screen_rect();
        let slot = window_decoration::tab_slot(
            rect,
            stacking::tabs_of(&self.frames, cap),
            stacking::tabs_of(&self.frames, cap).selected,
        );
        (slot.x + slot.w as i32 - 10, slot.y + 12)
    }

    fn origin(&self, cap: u64) -> (i32, i32) {
        let s = self.table.get(cap).unwrap();
        (s.x, s.y)
    }
}

const META: u32 = 0x7D;
const SHIFT: u32 = 0x2A;

#[test]
fn win_drag_onto_another_windows_tab_stacks_it_there() {
    let mut desk = Desk::new();
    let a = desk.window(7, 100, 100);
    let b = desk.window(8, 600, 400);
    desk.hold(META, KeyState::Pressed);
    let (from, onto) = (desk.tab(b), desk.tab(a));
    let button = |state| InputEvent::MouseButton {
        button: MouseButton::Left,
        state,
    };
    desk.route(button(KeyState::Pressed), from.0, from.1);
    desk.route(
        InputEvent::MouseMove {
            x: onto.0,
            y: onto.1,
            dx: 0,
            dy: 0,
        },
        onto.0,
        onto.1,
    );
    let feedback = desk
        .router
        .drag_feedback()
        .expect("a ghost follows the pointer");
    assert!(feedback.target.is_some(), "a's tabs are highlighted");
    assert_eq!(desk.origin(b), (600, 400), "nothing moves before the drop");
    desk.route(button(KeyState::Released), onto.0, onto.1);

    assert_eq!(desk.frames.frame_of(a), desk.frames.frame_of(b));
    assert_eq!(desk.origin(b), (100, 100), "b took a's place");
    assert_eq!(desk.router.drag_feedback(), None);
}

#[test]
fn win_drag_out_of_a_stack_detaches_where_dropped_and_esc_cancels() {
    let mut desk = Desk::new();
    let a = desk.window(7, 100, 100);
    let b = desk.window(8, 100, 100);
    let (from, onto) = (
        desk.frames.frame_of(b).unwrap(),
        desk.frames.frame_of(a).unwrap(),
    );
    desk.frames.stack(from, onto);
    stacking::sync(&desk.frames, &mut desk.table, onto);
    desk.hold(META, KeyState::Pressed);

    // Esc halfway: nothing changes.
    let start = desk.tab(b);
    desk.route(
        InputEvent::MouseButton {
            button: MouseButton::Left,
            state: KeyState::Pressed,
        },
        start.0,
        start.1,
    );
    desk.route(
        InputEvent::MouseMove {
            x: 700,
            y: 500,
            dx: 0,
            dy: 0,
        },
        700,
        500,
    );
    assert!(desk.router.cancel_stack_drag().is_some());
    desk.route(
        InputEvent::MouseButton {
            button: MouseButton::Left,
            state: KeyState::Released,
        },
        700,
        500,
    );
    assert_eq!(desk.frames.frame_of(a), desk.frames.frame_of(b));

    // Pressed 10px left of the first press: not a double click.
    let start = (start.0 - 10, start.1);
    desk.drag(start, (start.0 + 300, start.1 + 200));
    assert_ne!(desk.frames.frame_of(a), desk.frames.frame_of(b));
    assert_eq!(desk.origin(b), (400, 300), "dropped where the ghost was");
    assert_eq!(desk.origin(a), (100, 100), "the rest of the stack stays");
}

#[test]
fn a_plain_drag_moves_and_a_win_drag_of_a_lone_window_moves_on_drop() {
    let mut desk = Desk::new();
    let a = desk.window(7, 100, 100);
    let start = desk.tab(a);
    desk.drag(start, (start.0 + 50, start.1 + 20));
    assert_eq!(desk.origin(a), (150, 120));
    desk.hold(META, KeyState::Pressed);
    desk.hold(META, KeyState::Released);
    desk.hold(META, KeyState::Pressed);
    let start = desk.tab(a);
    desk.drag((start.0 - 10, start.1), (start.0, start.1 + 10));
    assert_eq!(desk.origin(a), (160, 130));
}

#[test]
fn shift_drag_slides_a_lone_tab_within_its_frame() {
    let mut desk = Desk::new();
    let a = desk.window(7, 100, 100);
    desk.hold(SHIFT, KeyState::Pressed);
    let start = desk.tab(a);
    desk.drag(start, (start.0 + 60, start.1 + 40));
    let frame = desk.frames.frame_of(a).unwrap();
    assert_eq!(desk.frames.tab_offset(frame), 60);
    assert_eq!(
        desk.origin(a),
        (100, 100),
        "sliding the tab does not move the window"
    );
    let start = desk.tab(a);
    desk.drag((start.0 - 10, start.1), (start.0 + 900, start.1));
    let max = window_decoration::max_tab_offset(desk.table.get(a).unwrap().screen_rect());
    assert_eq!(
        desk.frames.tab_offset(frame),
        max,
        "the tab stops at the frame's edge"
    );

    // Joining a stack puts the tabs back across the frame.
    desk.hold(SHIFT, KeyState::Released);
    let b = desk.window(8, 500, 400);
    let from = desk.frames.frame_of(b).unwrap();
    desk.frames.stack(from, frame);
    assert_eq!(stacking::tabs_of(&desk.frames, a).offset, 0);
}

#[test]
fn win_drag_beside_another_window_tiles_it_and_a_plain_drag_moves_the_group() {
    let mut desk = Desk::new();
    let a = desk.window(7, 100, 100);
    let b = desk.window(8, 700, 500);
    desk.hold(META, KeyState::Pressed);
    // a's visible frame ends at x = 401; drop b's frame 6px right of it.
    let from = desk.tab(b);
    let b_visible = window_decoration::frame_rect(desk.table.get(b).unwrap().screen_rect());
    let shift = (407 - b_visible.x, 100 - b_visible.y);
    desk.drag(from, (from.0 + shift.0, from.1 + shift.1));
    let b_visible = window_decoration::frame_rect(desk.table.get(b).unwrap().screen_rect());
    assert_eq!(b_visible.x, 401, "snapped flush against a");
    assert!(desk.frames.is_tiled(desk.frames.frame_of(a).unwrap()));

    desk.hold(META, KeyState::Released);
    let start = desk.tab(a);
    desk.drag((start.0 - 10, start.1), (start.0 + 10, start.1 + 30));
    assert_eq!(desk.origin(a), (120, 130));
    let moved = window_decoration::frame_rect(desk.table.get(b).unwrap().screen_rect());
    assert_eq!(
        (moved.x, moved.y),
        (b_visible.x + 20, b_visible.y + 30),
        "b moved with a"
    );
}

#[test]
fn maximizing_the_middle_of_a_tile_group_untiles_it() {
    let mut desk = Desk::new();
    let a = desk.window(7, 100, 100);
    let b = desk.window(8, 402, 100);
    let c = desk.window(9, 704, 100);
    let (fa, fb, fc) = (
        desk.frames.frame_of(a).unwrap(),
        desk.frames.frame_of(b).unwrap(),
        desk.frames.frame_of(c).unwrap(),
    );
    assert!(desk
        .frames
        .glue(fa, fb, crate::surface_table::frame::Side::Right));
    assert!(desk
        .frames
        .glue(fb, fc, crate::surface_table::frame::Side::Right));
    // Double click on b's title (now_ms is 0 for both presses).
    let title = desk.tab(b);
    let click = |desk: &mut Desk| {
        for state in [KeyState::Pressed, KeyState::Released] {
            desk.route(
                InputEvent::MouseButton {
                    button: MouseButton::Left,
                    state,
                },
                title.0,
                title.1,
            );
        }
    };
    click(&mut desk);
    click(&mut desk);
    assert!(
        desk.table.get(b).unwrap().is_configuring(),
        "b is asked to maximize"
    );
    assert!(!desk.frames.is_tiled(fb));
    assert!(
        !desk.frames.is_tiled(fa) && !desk.frames.is_tiled(fc),
        "a and c no longer touch"
    );
}
