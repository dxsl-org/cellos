//! ViSurface wrappers over the CellOS Linear icon set
//! (`ostd::display::linear::icons`).

use ostd::display::ViSurface;

pub use ostd::display::linear::icons::{paint_icon, LineIcon, ICON_SIZE};

use super::{bgra, linear_canvas};
use crate::canvas::Color;

/// Draw `icon` with its grid's top-left at `(x, y)`.
pub fn draw_icon(surf: &mut ViSurface, icon: LineIcon, x: i32, y: i32, color: Color) {
    paint_icon(&mut linear_canvas(surf), icon, x, y, bgra(color));
}

/// Draw `icon` centred in the box `(x, y, w, h)`, snapped to whole pixels.
pub fn draw_icon_centered(
    surf: &mut ViSurface,
    icon: LineIcon,
    (x, y, w, h): (i32, i32, u32, u32),
    color: Color,
) {
    let left = x + (w.saturating_sub(ICON_SIZE) / 2) as i32;
    let top = y + (h.saturating_sub(ICON_SIZE) / 2) as i32;
    draw_icon(surf, icon, left, top, color);
}
