//! What the window menu offers for one window, decided when it opens and
//! re-checked when an item runs (the screen may change while it is open).

use alloc::vec::Vec;
use wire::display::Rect;

use crate::stacking;
use crate::surface_table::{FrameRegistry, SurfaceTable};
use crate::window_decoration::{self, MenuAction, MenuItems};
use crate::z_order::ZOrder;

/// Windows nearer than this (between visible frames) are offered as stack targets.
const NEAR: i32 = 48;
/// Stack targets one menu offers at most.
const MAX_TARGETS: usize = 5;

/// Items for the window menu of the shown window `cap`.
pub(super) fn items_for(
    cap: u64,
    table: &SurfaceTable,
    frames: &FrameRegistry,
    z_order: &ZOrder,
) -> MenuItems {
    let tabs = frames
        .frame_of(cap)
        .map_or(1, |frame| frames.members(frame).len());
    let mut items = MenuItems::new();
    items.push(MenuAction::Maximize);
    items.push(MenuAction::Minimize);
    items.push(MenuAction::Close);
    if tabs > 1 {
        items.push(MenuAction::CloseAll);
    }
    let tiled = frames
        .frame_of(cap)
        .is_some_and(|frame| frames.is_tiled(frame));
    if tiled {
        items.push(MenuAction::CloseGroup);
    }
    for target in stack_targets(cap, table, frames, z_order) {
        items.push(MenuAction::StackOnto(target));
    }
    if tabs > 1 {
        items.push(MenuAction::Detach);
    }
    if tiled {
        items.push(MenuAction::Untile);
    }
    items
}

/// Shown windows of other frames that overlap `cap`'s frame or lie within
/// [`NEAR`] of it, nearest first. A window whose tab and body both lie under
/// one window above it is not offered: the user cannot see what they would
/// stack onto.
pub(super) fn stack_targets(
    cap: u64,
    table: &SurfaceTable,
    frames: &FrameRegistry,
    z_order: &ZOrder,
) -> Vec<u64> {
    let Some(own) = table.get(cap).map(|surface| visible(surface.screen_rect())) else {
        return Vec::new();
    };
    let own_frame = frames.frame_of(cap);
    let order: Vec<u64> = z_order.iter_bottom_to_top().collect();
    let mut targets: Vec<(i32, u64)> = order
        .iter()
        .enumerate()
        .filter(|&(_, &other)| {
            frames
                .frame_of(other)
                .is_some_and(|frame| Some(frame) != own_frame)
        })
        .filter_map(|(depth, &other)| {
            let surface = table.get(other).filter(|surface| surface.is_shown())?;
            let parts = window_decoration::visible_parts(
                surface.screen_rect(),
                stacking::tabs_of(frames, other),
            );
            let covered = order[depth + 1..].iter().any(|&above| {
                table.get(above).filter(|s| s.is_shown()).is_some_and(|s| {
                    let cover = window_decoration::visible_parts(
                        s.screen_rect(),
                        stacking::tabs_of(frames, above),
                    );
                    parts
                        .iter()
                        .all(|part| cover.iter().any(|&c| covers(c, *part)))
                })
            });
            let gap = gap(own, visible(surface.screen_rect()));
            (!covered && gap <= NEAR).then_some((gap, other))
        })
        .collect();
    targets.sort_by_key(|&(gap, _)| gap);
    targets
        .into_iter()
        .take(MAX_TARGETS)
        .map(|(_, cap)| cap)
        .collect()
}

/// Whether `target` may still take `source`'s stack when the item runs.
pub(super) fn can_stack(
    source: u64,
    target: u64,
    table: &SurfaceTable,
    frames: &FrameRegistry,
) -> bool {
    let shown = |cap: u64| table.get(cap).is_some_and(|surface| surface.is_shown());
    shown(source)
        && shown(target)
        && frames
            .frame_of(target)
            .is_some_and(|frame| Some(frame) != frames.frame_of(source))
}

fn visible(content: Rect) -> Rect {
    window_decoration::frame_rect(content)
}

/// Largest of the horizontal and vertical gaps between `a` and `b` (0 when they touch or overlap).
fn gap(a: Rect, b: Rect) -> i32 {
    let span = |start: i32, extent: u32| (i64::from(start), i64::from(start) + i64::from(extent));
    let ((ax0, ax1), (bx0, bx1)) = (span(a.x, a.w), span(b.x, b.w));
    let ((ay0, ay1), (by0, by1)) = (span(a.y, a.h), span(b.y, b.h));
    let dx = (bx0 - ax1).max(ax0 - bx1).max(0);
    let dy = (by0 - ay1).max(ay0 - by1).max(0);
    dx.max(dy).min(i64::from(i32::MAX)) as i32
}

fn covers(outer: Rect, inner: Rect) -> bool {
    let end = |start: i32, extent: u32| i64::from(start) + i64::from(extent);
    outer.x <= inner.x
        && outer.y <= inner.y
        && end(outer.x, outer.w) >= end(inner.x, inner.w)
        && end(outer.y, outer.h) >= end(inner.y, inner.h)
}

#[cfg(test)]
mod tests {
    use wire::display::SurfaceRole;

    use super::*;

    fn window(
        table: &mut SurfaceTable,
        frames: &mut FrameRegistry,
        z: &mut ZOrder,
        x: i32,
        y: i32,
    ) -> u64 {
        let cap = table
            .create(x, y, 200, 150, 7, SurfaceRole::Interactive)
            .unwrap();
        frames.adopt(cap, 7);
        z.push(cap);
        cap
    }

    #[test]
    fn stack_targets_are_nearby_visible_windows_nearest_first() {
        let (mut table, mut frames, mut z) =
            (SurfaceTable::new(), FrameRegistry::new(), ZOrder::new());
        let me = window(&mut table, &mut frames, &mut z, 100, 100);
        let far = window(&mut table, &mut frames, &mut z, 800, 600);
        let near = window(&mut table, &mut frames, &mut z, 340, 100);
        let overlapping = window(&mut table, &mut frames, &mut z, 150, 150);
        let covered = window(&mut table, &mut frames, &mut z, 160, 160);
        let cover = table
            .create(140, 130, 260, 230, 8, SurfaceRole::Interactive)
            .unwrap();
        frames.adopt(cover, 8);
        z.push(cover);
        z.raise(me);
        z.raise(overlapping);
        // `cover` hides `covered` entirely but sits below `overlapping` and `me`.
        let targets = stack_targets(me, &table, &frames, &z);
        // Equally near ones come in stacking order.
        assert_eq!(
            targets,
            [cover, overlapping, near],
            "covered and far ones are left out"
        );
        let _ = (far, covered);

        let items = items_for(me, &table, &frames, &z);
        assert_eq!(
            items
                .iter()
                .filter(|a| matches!(a, MenuAction::StackOnto(_)))
                .count(),
            3
        );
        assert!(
            !items
                .iter()
                .any(|a| matches!(a, MenuAction::Detach | MenuAction::CloseAll)),
            "one tab"
        );
    }

    #[test]
    fn a_window_whose_tab_peeks_out_is_still_offered() {
        let (mut table, mut frames, mut z) =
            (SurfaceTable::new(), FrameRegistry::new(), ZOrder::new());
        let behind = table
            .create(100, 100, 600, 300, 8, SurfaceRole::Interactive)
            .unwrap();
        frames.adopt(behind, 8);
        z.push(behind);
        // Same place, narrower: its 160px tab leaves the 240px tab behind visible.
        let me = window(&mut table, &mut frames, &mut z, 100, 100);
        table.get_mut(me).unwrap().w = 300;
        table.get_mut(me).unwrap().h = 300;
        let tall = table
            .create(100, 100, 600, 300, 9, SurfaceRole::Interactive)
            .unwrap();
        frames.adopt(tall, 9);
        z.push(tall);
        z.raise(me);
        assert_eq!(
            stack_targets(me, &table, &frames, &z),
            [tall],
            "behind is fully under tall"
        );
        z.raise(behind);
        z.raise(me);
        assert!(stack_targets(me, &table, &frames, &z).contains(&behind));
    }

    #[test]
    fn a_target_that_left_while_the_menu_was_open_is_refused() {
        let (mut table, mut frames, mut z) =
            (SurfaceTable::new(), FrameRegistry::new(), ZOrder::new());
        let me = window(&mut table, &mut frames, &mut z, 100, 100);
        let other = window(&mut table, &mut frames, &mut z, 150, 150);
        assert!(can_stack(me, other, &table, &frames));
        let (from, onto) = (
            frames.frame_of(other).unwrap(),
            frames.frame_of(me).unwrap(),
        );
        frames.stack(from, onto);
        assert!(!can_stack(me, other, &table, &frames), "same frame now");
        frames.remove_surface(other);
        table.remove(other);
        assert!(!can_stack(me, other, &table, &frames), "gone");
    }
}
