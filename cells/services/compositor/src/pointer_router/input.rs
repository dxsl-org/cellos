use wire::display::Rect;
use wire::input::{InputEvent, KeyState, MouseButton};

use crate::window_decoration::{self, Control, Hit};

use super::{
    actions, resize, stack_drag, target, window_menu, Capture, DragMode, OpenMenu, PointerRouter,
    RouteContext, DOUBLE_CLICK_MS, DOUBLE_CLICK_SLOP,
};
use crate::stacking;
use crate::tiling;

pub(super) fn route<F>(
    router: &mut PointerRouter,
    event: InputEvent,
    x: i32,
    y: i32,
    context: &mut RouteContext<'_, F>,
) where
    F: FnMut(Rect),
{
    match event {
        InputEvent::MouseMove { .. } => move_pointer(router, event, x, y, context),
        InputEvent::MouseButton { button, state } => {
            route_button(router, button, state, x, y, context)
        }
        InputEvent::MouseScroll { .. } => {
            if router.menu.is_some() {
                return;
            }
            if let Some(target) = target::at(x, y, context.table, context.frames, context.z_order)
                .filter(|target| target.hit == Hit::Content)
            {
                target::send_position(target, x, y);
                target::send_local(target, event);
            }
        }
        InputEvent::Key(_) => {}
    }
}

fn move_pointer<F>(
    router: &mut PointerRouter,
    event: InputEvent,
    x: i32,
    y: i32,
    context: &mut RouteContext<'_, F>,
) where
    F: FnMut(Rect),
{
    if let Some(menu) = router.menu {
        // The open menu owns the pointer: track its hovered item, nothing else.
        let hover = window_decoration::menu_item_at(menu.rect, &menu.items, x, y);
        if hover != menu.hover {
            router.menu = Some(OpenMenu { hover, ..menu });
            (context.damage)(menu.rect);
        }
        return;
    }
    match router.capture {
        Some(Capture::Client(cap)) => {
            if let Some(target) = target::for_cap(cap, context.table) {
                target::send_local(target, event);
            }
        }
        Some(Capture::Drag {
            cap,
            start_x,
            start_y,
            rect,
            mode: DragMode::Stack { .. },
        }) => stack_drag::track(router, cap, (start_x, start_y), rect, x, y, context),
        Some(Capture::Drag {
            cap,
            start_x,
            mode: DragMode::Slide { start_offset },
            ..
        }) => stack_drag::slide(cap, start_offset, x.saturating_sub(start_x), context),
        Some(Capture::Drag {
            cap,
            start_x,
            start_y,
            rect,
            mode: DragMode::Move,
        }) => {
            if let Some(surface) = context
                .table
                .get_mut(cap)
                .filter(|surface| surface.accepts_input())
            {
                let old = window_decoration::bounds(surface.screen_rect());
                let before = (surface.x, surface.y);
                surface.move_to(
                    rect.x.saturating_add(x.saturating_sub(start_x)),
                    rect.y.saturating_add(y.saturating_sub(start_y)),
                );
                let delta = (surface.x - before.0, surface.y - before.1);
                (context.damage)(old.union(&window_decoration::bounds(surface.screen_rect())));
                // The other tabs of its stack and the frames of its tile
                // group travel with it.
                stacking::sync_frame_of(context.frames, context.table, cap);
                if let Some(area) = tiling::follow(context.frames, context.table, cap, delta) {
                    (context.damage)(area);
                }
            }
        }
        Some(Capture::Resize {
            cap,
            start_x,
            start_y,
            rect,
            edge,
        }) => {
            let delta = (x.saturating_sub(start_x), y.saturating_sub(start_y));
            if !tiling::resize(context.frames, context.table, cap, rect, edge, delta) {
                resize::propose(cap, rect, edge, delta.0, delta.1, context.table);
            }
        }
        Some(Capture::Menu) => {}
        None => {
            if let Some(target) = target::at(x, y, context.table, context.frames, context.z_order)
                .filter(|target| target.hit == Hit::Content)
            {
                target::send_local(target, event);
            }
        }
    }
}

fn route_button<F>(
    router: &mut PointerRouter,
    button: MouseButton,
    state: KeyState,
    x: i32,
    y: i32,
    context: &mut RouteContext<'_, F>,
) where
    F: FnMut(Rect),
{
    let event = InputEvent::MouseButton { button, state };
    if let Some(menu) = router.menu {
        // Any press outside the menu dismisses it and goes nowhere else.
        if state == KeyState::Pressed {
            if contains(menu.rect, x, y) && button == MouseButton::Left {
                router.capture = Some(Capture::Menu);
            } else if let Some(area) = router.close_menu() {
                (context.damage)(area);
            }
            return;
        }
        if state == KeyState::Released && matches!(router.capture, Some(Capture::Menu)) {
            actions::release_in_menu(router, menu, x, y, context);
            return;
        }
    }
    if button != MouseButton::Left {
        if let Some(target) = target::at(x, y, context.table, context.frames, context.z_order)
            .filter(|target| target.hit == Hit::Content)
        {
            target::send_position(target, x, y);
            target::send_local(target, event);
        }
        return;
    }
    match state {
        KeyState::Pressed => press(router, event, x, y, context),
        KeyState::Released => match router.capture.take() {
            Some(Capture::Client(cap)) => {
                if let Some(target) = target::for_cap(cap, context.table) {
                    target::send_position(target, x, y);
                    target::send_local(target, event);
                }
            }
            Some(Capture::Drag {
                cap,
                start_x,
                start_y,
                mode: DragMode::Stack { target, glue },
                ..
            }) => {
                let delta = (x.saturating_sub(start_x), y.saturating_sub(start_y));
                stack_drag::drop(router, cap, delta, (target, glue), context);
            }
            _ => {}
        },
        KeyState::Repeated => {}
    }
}

fn press<F>(
    router: &mut PointerRouter,
    event: InputEvent,
    x: i32,
    y: i32,
    context: &mut RouteContext<'_, F>,
) where
    F: FnMut(Rect),
{
    let Some(target) = target::at(x, y, context.table, context.frames, context.z_order) else {
        return;
    };
    router.focused_owner = target.owner;
    let old = router
        .selected_cap
        .and_then(|cap| context.table.get(cap))
        .map(|surface| window_decoration::bounds(surface.screen_rect()));
    router.selected_cap = Some(target.cap);
    context.z_order.raise(target.cap);
    tiling::raise(context.frames, context.z_order, target.cap);
    let Some(rect) = context
        .table
        .get(target.cap)
        .map(|surface| surface.screen_rect())
    else {
        return;
    };
    let now = window_decoration::bounds(rect);
    (context.damage)(old.map_or(now, |prior| prior.union(&now)));

    router.capture = match target.hit {
        Hit::Content => {
            target::send_position(target, x, y);
            target::send_local(target, event);
            Some(Capture::Client(target.cap))
        }
        Hit::Title if is_double_click(router, target.cap, x, y, context.now_ms) => {
            router.last_title_press = None;
            actions::apply(router, target.cap, Control::Maximize, context);
            None
        }
        Hit::Tab(index) => {
            let shown = actions::show_tab(router, target.cap, index, context);
            // Win held: the picked tab is dragged out right away, as in Haiku.
            let stack = DragMode::Stack {
                target: None,
                glue: None,
            };
            match shown {
                Some(cap) if stack_drag::mode_for(router.modifiers, cap, context) == stack => {
                    Some(Capture::Drag {
                        cap,
                        start_x: x,
                        start_y: y,
                        rect,
                        mode: stack,
                    })
                }
                _ => None,
            }
        }
        Hit::Title => {
            router.last_title_press = Some((target.cap, context.now_ms, x, y));
            let mode = stack_drag::mode_for(router.modifiers, target.cap, context);
            Some(Capture::Drag {
                cap: target.cap,
                start_x: x,
                start_y: y,
                rect,
                mode,
            })
        }
        Hit::Resize(edge) => Some(Capture::Resize {
            cap: target.cap,
            start_x: x,
            start_y: y,
            rect,
            edge,
        }),
        Hit::Menu => {
            let (screen_w, screen_h) = context.screen;
            let tabs = stacking::tabs_of(context.frames, target.cap);
            let items =
                window_menu::items_for(target.cap, context.table, context.frames, context.z_order);
            let menu_rect = window_decoration::menu_rect(rect, tabs, &items, screen_w, screen_h);
            router.menu = Some(OpenMenu {
                cap: target.cap,
                rect: menu_rect,
                items,
                hover: window_decoration::menu_item_at(menu_rect, &items, x, y),
            });
            (context.damage)(window_decoration::menu_region(menu_rect));
            None
        }
    };
}

pub(super) fn is_double_click(router: &PointerRouter, cap: u64, x: i32, y: i32, now: u64) -> bool {
    router
        .last_title_press
        .is_some_and(|(pressed, at, px, py)| {
            pressed == cap
                && now.saturating_sub(at) <= DOUBLE_CLICK_MS
                && (x - px).abs() <= DOUBLE_CLICK_SLOP
                && (y - py).abs() <= DOUBLE_CLICK_SLOP
        })
}

fn contains(rect: Rect, x: i32, y: i32) -> bool {
    x >= rect.x
        && y >= rect.y
        && i64::from(x) < i64::from(rect.x) + i64::from(rect.w)
        && i64::from(y) < i64::from(rect.y) + i64::from(rect.h)
}
