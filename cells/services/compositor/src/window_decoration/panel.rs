//! Shape of shell Panel surfaces (taskbar, Spotlight).
//!
//! Policy: a Panel corner is rounded to [`RADIUS`] unless one of its two edges
//! lies on a screen edge, so a docked bar stays flush with the screen while a
//! floating popover reads as a box. Only a floating Panel (touching no screen
//! edge) casts a shadow. The shell outlines its panel with the same radius
//! (`ostd::display::linear::PANEL_RADIUS`), so the clipped corner and its
//! 1px border coincide.

use ostd::display::linear::{Radii, RoundRect};
use wire::display::Rect;

use super::{end, RADIUS, SHADOW_MARGIN};

/// The panel's visible shape and whether it floats.
pub fn panel_shape(rect: Rect, screen_w: u32, screen_h: u32) -> (RoundRect, bool) {
    let left = rect.x <= 0;
    let top = rect.y <= 0;
    let right = end(rect.x, rect.w) >= screen_w.min(i32::MAX as u32) as i32;
    let bottom = end(rect.y, rect.h) >= screen_h.min(i32::MAX as u32) as i32;
    let corner = |a: bool, b: bool| if a || b { 0 } else { RADIUS };
    let radii = Radii {
        top_left: corner(top, left),
        top_right: corner(top, right),
        bottom_right: corner(bottom, right),
        bottom_left: corner(bottom, left),
    };
    let floating = !(left || top || right || bottom);
    (
        RoundRect::new(rect.x, rect.y, rect.w, rect.h, radii),
        floating,
    )
}

/// Everything a Panel may paint, shadow included.
pub fn panel_region(rect: Rect) -> Rect {
    Rect {
        x: rect.x.saturating_sub(SHADOW_MARGIN),
        y: rect.y.saturating_sub(SHADOW_MARGIN),
        w: rect.w.saturating_add((SHADOW_MARGIN * 2) as u32),
        h: rect.h.saturating_add((SHADOW_MARGIN * 2) as u32),
    }
}
