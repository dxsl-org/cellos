//! Haiku's tab gestures. Win (Meta, Haiku's Option key) + drag of a tab:
//! drop it on another window's tabs to stack it there, next to another
//! window's edge to tile it there, anywhere else to pull it out of its stack
//! and tile group (or just move a lone window). Shift + drag of a lone tab
//! slides it along the frame's top edge.

use wire::display::Rect;
use wire::input::{KeyEvent, KeyState, Modifiers};

use super::{Capture, PointerRouter, RouteContext};
use crate::stacking;
use crate::tiling::{self, Glue};
use crate::window_decoration;

/// How far above and below a tab strip a dragged tab still lands on it.
const STRIP_SLACK: i32 = 8;
/// Raw key codes (Linux EV_KEY) of the keys the gestures read.
const LEFT_META: u32 = 0x7D;
const RIGHT_META: u32 = 0x7E;
const LEFT_SHIFT: u32 = 0x2A;
const RIGHT_SHIFT: u32 = 0x36;

/// What a press on a tab starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragMode {
    /// Move the frame.
    Move,
    /// Win+drag: the frame stays put; a ghost shows where it would land,
    /// `target` the window whose tabs it would join, else `glue` the edge
    /// it would tile to.
    Stack {
        target: Option<u64>,
        glue: Option<Glue>,
    },
    /// Shift+drag of a lone tab, which started `start_offset` from the left.
    Slide { start_offset: u32 },
}

/// What a Win+drag shows: the visible frame where it would land, and the
/// tab strip it would join.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DragFeedback {
    pub ghost: Rect,
    pub target: Option<Rect>,
}

impl DragFeedback {
    /// Everything the feedback paints (1px outlines, 2px around the target).
    pub fn area(&self) -> Rect {
        let ghost = grow(self.ghost, 1);
        self.target
            .map_or(ghost, |target| ghost.union(&grow(target, 2)))
    }
}

/// Modifier state after `key`, from the event's own flags and the key itself
/// (the press of a modifier may not yet be in its own flags).
pub(super) fn modifiers_after(key: &KeyEvent) -> Modifiers {
    let mut modifiers = key.modifiers;
    let held = key.state != KeyState::Released;
    for (codes, flag) in [
        ([LEFT_META, RIGHT_META], Modifiers::META),
        ([LEFT_SHIFT, RIGHT_SHIFT], Modifiers::SHIFT),
    ] {
        if codes.contains(&key.scancode) {
            if held {
                modifiers.set(flag);
            } else {
                modifiers.clear(flag);
            }
        }
    }
    modifiers
}

/// A Shift/Ctrl/Alt/Win key event on its own: a modifier transition sent
/// to the compositor only, never to be forwarded to an app.
pub fn is_modifier_key(key: &KeyEvent) -> bool {
    key.keysym == wire::input::KeySym::Unknown
        && matches!(
            key.scancode,
            0x2A | 0x36 | 0x1D | 0x61 | 0x38 | 0x64 | LEFT_META | RIGHT_META
        )
}

/// The drag a press on the shown tab of `cap` starts under `modifiers`.
pub(super) fn mode_for<F>(
    modifiers: Modifiers,
    cap: u64,
    context: &RouteContext<'_, F>,
) -> DragMode {
    if modifiers.contains(Modifiers::META) {
        DragMode::Stack {
            target: None,
            glue: None,
        }
    } else if modifiers.contains(Modifiers::SHIFT)
        && stacking::tabs_of(context.frames, cap).count == 1
    {
        DragMode::Slide {
            start_offset: stacking::tabs_of(context.frames, cap).offset,
        }
    } else {
        DragMode::Move
    }
}

/// Pointer at `(x, y)` during a Win+drag of `cap` that started at `start`
/// with the frame at `rect`: update the ghost and the target.
pub(super) fn track<F>(
    router: &mut PointerRouter,
    cap: u64,
    start: (i32, i32),
    rect: Rect,
    x: i32,
    y: i32,
    context: &mut RouteContext<'_, F>,
) where
    F: FnMut(Rect),
{
    let target = target_at(cap, x, y, context);
    let moved = Rect {
        x: rect.x.saturating_add(x - start.0),
        y: rect.y.saturating_add(y - start.1),
        ..rect
    };
    let glue = target
        .is_none()
        .then(|| tiling::glue_at(context.frames, context.table, context.z_order, cap, moved))
        .flatten();
    let feedback = DragFeedback {
        ghost: window_decoration::frame_rect(glue.map_or(moved, |glue| glue.content)),
        target: target
            .and_then(|target| strip_of(target, context))
            .or(glue.map(|glue| glue.edge)),
    };
    if let Some(Capture::Drag {
        mode: DragMode::Stack {
            target: held,
            glue: edge,
        },
        ..
    }) = &mut router.capture
    {
        *held = target;
        *edge = glue;
    }
    show(router, Some(feedback), context);
}

/// Release of a Win+drag of `cap`: stack onto `target`; else tile to `glue`
/// or just land where the ghost was. A tab leaves its stack and the frame
/// its tile group unless it is stacked.
pub(super) fn drop<F>(
    router: &mut PointerRouter,
    cap: u64,
    delta: (i32, i32),
    (target, glue): (Option<u64>, Option<Glue>),
    context: &mut RouteContext<'_, F>,
) where
    F: FnMut(Rect),
{
    show(router, None, context);
    let stack_onto = target.filter(|&target| {
        super::window_menu::can_stack(cap, target, context.table, context.frames)
    });
    if let Some(target) = stack_onto {
        let area = stacking::stack(context.frames, context.table, context.z_order, cap, target);
        super::actions::settle(router, cap, area, context);
        return;
    }
    if target.is_some() {
        // The tabs the user aimed at went away before the drop: nothing moves.
        return;
    }
    if let Some(frame) = context.frames.frame_of(cap) {
        context.frames.untile(frame);
    }
    let delta = match (glue, context.table.get(cap)) {
        (Some(glue), Some(s)) => (glue.content.x - s.x, glue.content.y - s.y),
        _ => delta,
    };
    let area = if stacking::tabs_of(context.frames, cap).count > 1 {
        let at = context
            .table
            .get(cap)
            .map(|s| (s.x.saturating_add(delta.0), s.y.saturating_add(delta.1)));
        stacking::detach(
            context.frames,
            context.table,
            context.z_order,
            cap,
            context.screen,
            at,
        )
    } else {
        move_frame(cap, delta, context)
    };
    if let Some(glue) = glue {
        tiling::glue(context.frames, cap, glue);
    }
    // The window the user carried ends up on top with the keyboard.
    super::actions::settle(router, cap, area, context);
}

/// Esc during a Win+drag: nothing moves. Returns whether one was cancelled.
pub(super) fn cancel(router: &mut PointerRouter) -> Option<Rect> {
    if !matches!(
        router.capture,
        Some(Capture::Drag {
            mode: DragMode::Stack { .. },
            ..
        })
    ) {
        return None;
    }
    router.capture = None;
    router.feedback.take().map(|feedback| feedback.area())
}

/// Shift+drag: slide the lone tab of `cap` by `dx` from where it started.
pub(super) fn slide<F>(cap: u64, start_offset: u32, dx: i32, context: &mut RouteContext<'_, F>)
where
    F: FnMut(Rect),
{
    let Some((frame, rect)) = context
        .frames
        .frame_of(cap)
        .zip(context.table.get(cap).map(|s| s.screen_rect()))
    else {
        return;
    };
    let offset = (i64::from(start_offset) + i64::from(dx))
        .clamp(0, i64::from(window_decoration::max_tab_offset(rect)));
    if context.frames.tab_offset(frame) != offset as u32 {
        context.frames.set_tab_offset(frame, offset as u32);
        (context.damage)(window_decoration::bounds(rect));
    }
}

fn move_frame<F>(cap: u64, delta: (i32, i32), context: &mut RouteContext<'_, F>) -> Option<Rect> {
    let surface = context.table.get_mut(cap)?;
    let before = window_decoration::bounds(surface.screen_rect());
    surface.move_to(
        surface.x.saturating_add(delta.0),
        surface.y.saturating_add(delta.1),
    );
    let after = window_decoration::bounds(surface.screen_rect());
    stacking::sync_frame_of(context.frames, context.table, cap);
    Some(before.union(&after))
}

/// The topmost shown window of another frame whose tab strip is under `(x, y)`.
fn target_at<F>(cap: u64, x: i32, y: i32, context: &RouteContext<'_, F>) -> Option<u64> {
    let own = context.frames.frame_of(cap);
    context.z_order.iter_top_to_bottom().find(|&other| {
        context
            .frames
            .frame_of(other)
            .is_some_and(|frame| Some(frame) != own)
            && strip_of(other, context).is_some_and(|strip| {
                x >= strip.x
                    && i64::from(x) < i64::from(strip.x) + i64::from(strip.w)
                    && y >= strip.y - STRIP_SLACK
                    && i64::from(y)
                        < i64::from(strip.y) + i64::from(strip.h) + i64::from(STRIP_SLACK)
            })
    })
}

/// The tab strip of the shown window `cap`.
fn strip_of<F>(cap: u64, context: &RouteContext<'_, F>) -> Option<Rect> {
    let surface = context
        .table
        .get(cap)
        .filter(|surface| surface.is_shown())?;
    Some(window_decoration::tab_rect(
        surface.screen_rect(),
        stacking::tabs_of(context.frames, cap),
    ))
}

fn show<F>(
    router: &mut PointerRouter,
    next: Option<DragFeedback>,
    context: &mut RouteContext<'_, F>,
) where
    F: FnMut(Rect),
{
    if router.feedback == next {
        return;
    }
    let area = [router.feedback, next]
        .into_iter()
        .flatten()
        .map(|f| f.area())
        .reduce(|a, b| a.union(&b));
    router.feedback = next;
    if let Some(area) = area {
        (context.damage)(area);
    }
}

fn grow(rect: Rect, by: i32) -> Rect {
    Rect {
        x: rect.x.saturating_sub(by),
        y: rect.y.saturating_sub(by),
        w: rect.w.saturating_add(by as u32 * 2),
        h: rect.h.saturating_add(by as u32 * 2),
    }
}
