use api::display::Rect;
use api::input::{InputEvent, KeyState, MouseButton};

use crate::surface_table::SurfaceTable;
use crate::window_decoration::{self, Hit};
use crate::z_order::ZOrder;

use super::{control, resize, target, Capture, PointerRouter};

pub(super) fn route<F>(
    router: &mut PointerRouter,
    event: InputEvent,
    x: i32,
    y: i32,
    maximum: Rect,
    table: &mut SurfaceTable,
    z_order: &mut ZOrder,
    damage: &mut F,
) where
    F: FnMut(Rect),
{
    match event {
        InputEvent::MouseMove { .. } => move_pointer(router, event, x, y, table, z_order, damage),
        InputEvent::MouseButton { button, state } => route_button(
            router, event, button, state, x, y, maximum, table, z_order, damage,
        ),
        InputEvent::MouseScroll { .. } => {
            if let Some(target) =
                target::at(x, y, table, z_order).filter(|target| target.hit == Hit::Content)
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
    table: &mut SurfaceTable,
    z_order: &mut ZOrder,
    damage: &mut F,
) where
    F: FnMut(Rect),
{
    match router.capture {
        Some(Capture::Client(cap)) => {
            if let Some(target) = target::for_cap(cap, table) {
                target::send_local(target, event);
            }
        }
        Some(Capture::Drag {
            cap,
            start_x,
            start_y,
            rect,
        }) => {
            if let Some(surface) = table.get_mut(cap).filter(|surface| surface.accepts_input()) {
                let old = window_decoration::bounds(surface.screen_rect());
                surface.move_to(
                    rect.x.saturating_add(x.saturating_sub(start_x)),
                    rect.y.saturating_add(y.saturating_sub(start_y)),
                );
                damage(old.union(&window_decoration::bounds(surface.screen_rect())));
            }
        }
        Some(Capture::Resize {
            cap,
            start_x,
            start_y,
            rect,
            edge,
        }) => resize::propose(
            cap,
            rect,
            edge,
            x.saturating_sub(start_x),
            y.saturating_sub(start_y),
            table,
        ),
        Some(Capture::Control { .. }) => {}
        None => {
            if let Some(target) =
                target::at(x, y, table, z_order).filter(|target| target.hit == Hit::Content)
            {
                target::send_local(target, event);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn route_button<F>(
    router: &mut PointerRouter,
    event: InputEvent,
    button: MouseButton,
    state: KeyState,
    x: i32,
    y: i32,
    maximum: Rect,
    table: &mut SurfaceTable,
    z_order: &mut ZOrder,
    damage: &mut F,
) where
    F: FnMut(Rect),
{
    if button != MouseButton::Left {
        if let Some(target) =
            target::at(x, y, table, z_order).filter(|target| target.hit == Hit::Content)
        {
            target::send_position(target, x, y);
            target::send_local(target, event);
        }
        return;
    }
    match state {
        KeyState::Pressed => {
            let Some(target) = target::at(x, y, table, z_order) else {
                return;
            };
            router.focused_owner = target.owner;
            let old = router
                .selected_cap
                .and_then(|cap| table.get(cap))
                .map(|surface| window_decoration::bounds(surface.screen_rect()));
            router.selected_cap = Some(target.cap);
            z_order.raise(target.cap);
            if let Some(surface) = table.get(target.cap) {
                damage(
                    old.map_or(window_decoration::bounds(surface.screen_rect()), |prior| {
                        prior.union(&window_decoration::bounds(surface.screen_rect()))
                    }),
                );
            }
            router.capture = Some(match target.hit {
                Hit::Content => {
                    target::send_position(target, x, y);
                    target::send_local(target, event);
                    Capture::Client(target.cap)
                }
                Hit::Title => Capture::Drag {
                    cap: target.cap,
                    start_x: x,
                    start_y: y,
                    rect: table.get(target.cap).unwrap().screen_rect(),
                },
                Hit::Resize(edge) => Capture::Resize {
                    cap: target.cap,
                    start_x: x,
                    start_y: y,
                    rect: table.get(target.cap).unwrap().screen_rect(),
                    edge,
                },
                Hit::Control(control) => Capture::Control {
                    cap: target.cap,
                    control,
                },
            });
        }
        KeyState::Released => match router.capture.take() {
            Some(Capture::Client(cap)) => {
                if let Some(target) = target::for_cap(cap, table) {
                    target::send_position(target, x, y);
                    target::send_local(target, event);
                }
            }
            Some(Capture::Control { cap, control }) if matches!(target::at(x, y, table, z_order), Some(target) if target.cap == cap && target.hit == Hit::Control(control)) => {
                if control::apply(cap, control, maximum, table, damage) {
                    router.disable(cap);
                }
            }
            _ => {}
        },
        KeyState::Repeated => {}
    }
}
