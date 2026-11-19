//! Linear-style raster primitives shared by the compositor and ViUI.
//!
//! Contract: every stroke is exactly one device pixel wide and every edge lands
//! on the pixel grid (no anti-aliasing), so window chrome, boxes and icons stay
//! sharp at any position, scale step and background. A rounded shape derives its
//! fill and its outline from the same per-row inset function, so the outline is
//! exactly the 4-connected boundary of the fill.
//!
//! Buffers are 32-bit BGRA rows with a byte stride. Opaque colors are copied;
//! translucent colors blend with the destination, as does the drop shadow
//! ([`Shadow`]).

mod canvas;
pub mod icons;
mod shadow;
#[cfg(test)]
mod tests;

pub use canvas::Canvas;
pub use shadow::Shadow;

/// One pixel in buffer byte order: blue, green, red, alpha.
pub type Bgra = [u8; 4];

/// Corner radius of buttons, badges, rows and other controls (CellOS Linear).
pub const CONTROL_RADIUS: u32 = 4;
/// Corner radius of window frames, panels, cards and popovers (CellOS Linear).
/// The compositor rounds floating Panel surfaces with this radius, so a shell
/// that outlines its panel with the same radius gets an unbroken border.
pub const PANEL_RADIUS: u32 = 6;

/// Per-corner radii in pixels. Each radius is clamped to half the shape's
/// shorter side when used.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Radii {
    pub top_left: u32,
    pub top_right: u32,
    pub bottom_right: u32,
    pub bottom_left: u32,
}

impl Radii {
    pub const fn all(r: u32) -> Self {
        Self {
            top_left: r,
            top_right: r,
            bottom_right: r,
            bottom_left: r,
        }
    }
}

/// Axis-aligned rectangle with independently rounded corners.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoundRect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
    pub radii: Radii,
}

impl RoundRect {
    pub const fn new(x: i32, y: i32, w: u32, h: u32, radii: Radii) -> Self {
        Self { x, y, w, h, radii }
    }

    fn clamp(&self, r: u32) -> u32 {
        r.min(self.w / 2).min(self.h / 2)
    }

    /// Pixels left uncovered at the start and at the end of `row` (0 = top).
    pub fn insets(&self, row: u32) -> (u32, u32) {
        if row >= self.h {
            return (0, 0);
        }
        let down = self.h - 1 - row;
        let left = corner_inset(self.clamp(self.radii.top_left), row)
            .max(corner_inset(self.clamp(self.radii.bottom_left), down));
        let right = corner_inset(self.clamp(self.radii.top_right), row)
            .max(corner_inset(self.clamp(self.radii.bottom_right), down));
        (left, right)
    }

    /// Outline pixels of `row`, relative to `x`: the half-open runs
    /// `[left.0, left.1)` and `[right.0, right.1)`. Top and bottom rows are one run.
    pub fn outline(&self, row: u32) -> ((u32, u32), (u32, u32)) {
        let (li, ri) = self.insets(row);
        let right_edge = self.w.saturating_sub(ri).max(li);
        if row == 0 || row + 1 >= self.h {
            return ((li, right_edge), (right_edge, right_edge));
        }
        // The row nearer the closest horizontal edge has the wider inset; the run
        // bridges to it so diagonal steps stay 4-connected.
        let toward = if row < self.h / 2 { row - 1 } else { row + 1 };
        let (lo, ro) = self.insets(toward);
        let left_end = lo.max(li + 1).min(right_edge);
        let right_start = self
            .w
            .saturating_sub(ro)
            .min(right_edge.saturating_sub(1))
            .max(li);
        ((li, left_end), (right_start, right_edge))
    }
}

/// Horizontal inset of row `row` (0 = the edge row) of a quarter circle of
/// radius `r` sampled at pixel centers. Rows at or beyond `r` have no inset.
pub const fn corner_inset(r: u32, row: u32) -> u32 {
    if row >= r {
        return 0;
    }
    // Doubled coordinates keep the half-pixel center offset integral:
    // inset = r - round(sqrt(r^2 - (r - row - 0.5)^2)).
    let d = 2 * (r - row) - 1;
    r - isqrt(4 * r * r - d * d).div_ceil(2)
}

pub(crate) const fn isqrt(n: u32) -> u32 {
    if n < 2 {
        return n;
    }
    let mut x = n;
    let mut y = n / 2 + (n & 1);
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}
