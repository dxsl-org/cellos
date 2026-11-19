//! Window actions the pointer starts: window-menu items and stacked tabs.

use wire::display::{Rect, WindowState};

use super::{control, window_menu, OpenMenu, PointerRouter, RouteContext};
use crate::stacking;
use crate::surface_table::SurfaceTable;
use crate::tiling;
use crate::window_decoration::{self, Control, MenuAction};

/// Release of a press that started inside the menu: run the item under the
/// pointer and close the menu; a release on padding keeps it open.
pub(super) fn release_in_menu<F>(
    router: &mut PointerRouter,
    menu: OpenMenu,
    x: i32,
    y: i32,
    context: &mut RouteContext<'_, F>,
) where
    F: FnMut(Rect),
{
    router.capture = None;
    let Some(action) = window_decoration::menu_item_at(menu.rect, &menu.items, x, y)
        .and_then(|index| menu.items.get(index))
    else {
        return;
    };
    if let Some(area) = router.close_menu() {
        (context.damage)(area);
    }
    let cap = menu.cap;
    match action {
        MenuAction::Maximize => apply(router, cap, Control::Maximize, context),
        MenuAction::Minimize => {
            // A tile group hides as one, and comes back as one.
            let group = tiling::group_windows(context.frames, cap);
            if group.len() > 1 {
                let frames: alloc::vec::Vec<u32> = group
                    .iter()
                    .filter_map(|&w| context.frames.frame_of(w))
                    .collect();
                context.frames.mark_group_minimized(&frames);
            }
            for other in group.into_iter().filter(|&o| o != cap) {
                apply(router, other, Control::Minimize, context);
            }
            apply(router, cap, Control::Minimize, context);
        }
        MenuAction::Untile => untile(cap, context),
        MenuAction::Close => apply(router, cap, Control::Close, context),
        MenuAction::StackOnto(target) => {
            // The screen may have changed while the menu was open.
            if !window_menu::can_stack(cap, target, context.table, context.frames) {
                return;
            }
            let area = stacking::stack(context.frames, context.table, context.z_order, cap, target);
            settle(router, cap, area, context);
        }
        MenuAction::Detach => {
            let area = stacking::detach(
                context.frames,
                context.table,
                context.z_order,
                cap,
                context.screen,
                None,
            );
            settle(router, cap, area, context);
        }
        MenuAction::CloseAll => {
            router.disable(cap);
            if let Some(area) =
                stacking::close_all(context.frames, context.table, context.z_order, cap)
            {
                (context.damage)(area);
            }
            if let Some(shown) = context
                .frames
                .frame_of(cap)
                .and_then(|frame| context.frames.selected_cap(frame))
            {
                focus(router, shown, context.table);
            }
        }
        MenuAction::CloseGroup => {
            router.disable(cap);
            for window in tiling::group_windows(context.frames, cap) {
                if let Some(area) =
                    stacking::close_all(context.frames, context.table, context.z_order, window)
                {
                    (context.damage)(area);
                }
            }
        }
    }
}

/// `cap` moved between frames: repaint, put it on top and give it focus.
pub(super) fn settle<F>(
    router: &mut PointerRouter,
    cap: u64,
    area: Option<Rect>,
    context: &mut RouteContext<'_, F>,
) where
    F: FnMut(Rect),
{
    let Some(area) = area else {
        return;
    };
    (context.damage)(area);
    context.z_order.raise(cap);
    focus(router, cap, context.table);
}

/// Take the frame of `cap` out of its tile group.
fn untile<F>(cap: u64, context: &mut RouteContext<'_, F>)
where
    F: FnMut(Rect),
{
    if let Some(frame) = context.frames.frame_of(cap) {
        context.frames.untile(frame);
    }
}

/// Run a window action. A window that stops taking input loses focus; when
/// it was the shown tab of a stack, the tab now shown takes focus instead.
/// Maximizing a tiled window takes it out of its tile group first.
pub(super) fn apply<F>(
    router: &mut PointerRouter,
    cap: u64,
    action: Control,
    context: &mut RouteContext<'_, F>,
) where
    F: FnMut(Rect),
{
    let maximizing = action == Control::Maximize
        && context
            .table
            .get(cap)
            .is_some_and(|s| s.state() == WindowState::Normal);
    if maximizing {
        untile(cap, context);
    }
    if action == Control::Close {
        // The user closes this tab by hand: any "Close all" here stops.
        if let Some(frame) = context.frames.frame_of(cap) {
            context.frames.clear_close_queue(frame);
        }
    }
    if !control::apply(
        cap,
        action,
        context.maximum,
        context.table,
        &mut context.damage,
    ) {
        return;
    }
    router.disable(cap);
    if action == Control::Close {
        if let Some((shown, area)) =
            stacking::show_next(context.frames, context.table, context.z_order, cap)
        {
            (context.damage)(area);
            focus(router, shown, context.table);
        }
    }
}

/// The user picked tab `index` of the stack showing `cap`. A tab that is
/// closing (or otherwise off screen) cannot be shown.
pub(super) fn show_tab<F>(
    router: &mut PointerRouter,
    cap: u64,
    index: usize,
    context: &mut RouteContext<'_, F>,
) -> Option<u64>
where
    F: FnMut(Rect),
{
    let member = context
        .frames
        .frame_of(cap)
        .and_then(|frame| context.frames.members(frame).get(index).copied())
        .filter(|&member| {
            context
                .table
                .get(member)
                .is_some_and(|s| matches!(s.state(), WindowState::Normal | WindowState::Maximized))
        })?;
    if let Some(frame) = context.frames.frame_of(member) {
        context.frames.clear_close_queue(frame);
    }
    if let Some(area) = stacking::select(context.frames, context.table, context.z_order, member) {
        (context.damage)(area);
    }
    focus(router, member, context.table);
    Some(member)
}

fn focus(router: &mut PointerRouter, cap: u64, table: &SurfaceTable) {
    if let Some(surface) = table.get(cap).filter(|surface| surface.accepts_input()) {
        router.focused_owner = surface.owner;
        router.selected_cap = Some(cap);
    }
}
