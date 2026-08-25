//! Damage aggregation and ordered scanout composition.

use api::display::Rect;

use crate::framebuffer::ScreenFb;
use crate::surface_table::SurfaceTable;
use crate::window_decoration;
use crate::z_order::ZOrder;

/// Reblend every surface intersecting the dirty region, then paint each surface's
/// compositor-owned frame and controls in stack order before flushing once.
pub fn render_frame(
    fb: &mut ScreenFb,
    table: &mut SurfaceTable,
    z_order: &ZOrder,
    extra_dirty: Option<Rect>,
    selected_cap: Option<u64>,
    cursor_x: i32,
    cursor_y: i32,
) -> Option<Rect> {
    let mut dirty = extra_dirty;
    for cap in z_order.iter_bottom_to_top() {
        let Some(surface) = table.get(cap) else {
            continue;
        };
        if !surface.is_visible_for_paint() {
            continue;
        }
        let Some(damage) = surface.damage else {
            continue;
        };
        let content_damage = Rect {
            x: surface.x.saturating_add(damage.x),
            y: surface.y.saturating_add(damage.y),
            w: damage.w,
            h: damage.h,
        };
        let screen_damage = if surface.is_window_managed() {
            window_decoration::bounds(surface.screen_rect())
        } else {
            content_damage
        };
        dirty = Some(dirty.map_or(screen_damage, |current| current.union(&screen_damage)));
    }
    let dirty = dirty?;
    fb.clear_rect(dirty);
    for cap in z_order.iter_bottom_to_top() {
        let Some(surface) = table.get(cap) else {
            continue;
        };
        if !surface.is_visible_for_paint() {
            continue;
        }
        if surface.screen_rect().intersects(&dirty) {
            fb.blit_surface(surface);
        }
        if surface.is_window_managed() {
            fb.paint_window_decoration(surface.screen_rect(), dirty, Some(cap) == selected_cap);
        }
    }
    for cap in z_order.iter_bottom_to_top() {
        if let Some(surface) = table.get_mut(cap) {
            surface.clear_damage();
        }
    }
    fb.composite_cursor(cursor_x, cursor_y, dirty);
    fb.flush_rect(dirty);
    Some(dirty)
}
