//! Soft drop shadow.

use super::{isqrt, Bgra, Canvas, RoundRect};

/// Soft drop shadow, alpha-blended over what is already painted.
///
/// The caller must have painted everything below the caster inside the clip
/// first (the compositor repaints its dirty region bottom-to-top), so each
/// pixel is blended exactly once per frame. Opacity falls off as `(1 - t)^2`
/// from `opacity / 255` at the caster's outline to zero at `spread` pixels;
/// a per-call table indexed by distance (1/16 px) holds it, so a ring pixel
/// costs one integer square root and one blend. The shadow shape is the
/// caster moved by `(offset_x, offset_y)`; pixels of the caster itself are
/// never written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shadow {
    pub color: Bgra,
    pub opacity: u8,
    /// Fade distance in pixels; values above [`MAX_SPREAD`] are clamped.
    pub spread: u32,
    pub offset_x: i32,
    pub offset_y: i32,
}

pub const MAX_SPREAD: u32 = 32;
/// Callers cast one window (tab + body) at a time; a fixed bound keeps the
/// per-row interior spans on the stack.
const MAX_CASTERS: usize = 4;
const STEPS: usize = (MAX_SPREAD * 16) as usize + 1;

impl Canvas<'_> {
    /// Blend `shadow` cast by the union of `casters` (at most four shapes).
    pub fn drop_shadow(&mut self, casters: &[RoundRect], shadow: &Shadow) {
        let casters = &casters[..casters.len().min(MAX_CASTERS)];
        let spread = shadow.spread.min(MAX_SPREAD);
        if casters.is_empty() || shadow.opacity == 0 || spread == 0 {
            return;
        }
        let ramp = ramp(shadow.opacity, spread);
        let reach = i64::from(spread);
        let offset = (i64::from(shadow.offset_x), i64::from(shadow.offset_y));
        let (mut rx0, mut ry0, mut rx1, mut ry1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
        for c in casters {
            rx0 = rx0.min(i64::from(c.x) + offset.0 - reach);
            ry0 = ry0.min(i64::from(c.y) + offset.1 - reach);
            rx1 = rx1.max(i64::from(c.x) + offset.0 + i64::from(c.w) + reach);
            ry1 = ry1.max(i64::from(c.y) + offset.1 + i64::from(c.h) + reach);
        }
        let (rx0, ry0) = (rx0.max(self.x0), ry0.max(self.y0));
        let (rx1, ry1) = (rx1.min(self.x1), ry1.min(self.y1));

        for y in ry0..ry1 {
            let mut interiors = [(0i64, 0i64); MAX_CASTERS];
            let mut count = 0;
            for c in casters {
                let row = y - i64::from(c.y);
                if row < 0 || row >= i64::from(c.h) {
                    continue;
                }
                let (li, ri) = c.insets(row as u32);
                let x = i64::from(c.x);
                interiors[count] = (x + i64::from(li), x + i64::from(c.w) - i64::from(ri));
                count += 1;
            }
            let interiors = &mut interiors[..count];
            interiors.sort_unstable();
            let mut x = rx0;
            for &(start, end) in interiors.iter() {
                if start > x {
                    self.shade_run(y, x, start.min(rx1), casters, offset, &ramp, shadow.color);
                }
                x = x.max(end);
            }
            if x < rx1 {
                self.shade_run(y, x, rx1, casters, offset, &ramp, shadow.color);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn shade_run(
        &mut self,
        y: i64,
        x0: i64,
        x1: i64,
        casters: &[RoundRect],
        offset: (i64, i64),
        ramp: &[u8; STEPS],
        color: Bgra,
    ) {
        let last = (ramp.len() - 1) as i64;
        for x in x0..x1 {
            let d16 = casters
                .iter()
                .map(|c| distance16(c, offset, x, y))
                .min()
                .unwrap_or(i64::MAX);
            let alpha = ramp[d16.min(last) as usize];
            if alpha > 0 {
                self.pixel(x, y, [color[0], color[1], color[2], alpha]);
            }
        }
    }
}

/// Opacity per distance step (1/16 px): `opacity * (1 - d / spread)^2`, zero
/// from `spread` on (the table tail stays zero).
fn ramp(opacity: u8, spread: u32) -> [u8; STEPS] {
    let mut out = [0u8; STEPS];
    let span = i64::from(spread) * 16;
    for (d, alpha) in out.iter_mut().enumerate().take(span as usize) {
        let left = span - d as i64;
        *alpha = (i64::from(opacity) * left * left / (span * span)) as u8;
    }
    out
}

/// Distance (1/16 px, 0 inside) from pixel `(x, y)` to the caster moved by
/// `offset`, measured from its rounded outline.
fn distance16(c: &RoundRect, offset: (i64, i64), x: i64, y: i64) -> i64 {
    let (left, top) = (i64::from(c.x) + offset.0, i64::from(c.y) + offset.1);
    let (w, h) = (i64::from(c.w), i64::from(c.h));
    let west = 2 * x < 2 * left + w - 1;
    let north = 2 * y < 2 * top + h - 1;
    let radius = match (north, west) {
        (true, true) => c.radii.top_left,
        (true, false) => c.radii.top_right,
        (false, false) => c.radii.bottom_right,
        (false, true) => c.radii.bottom_left,
    };
    let r = i64::from(radius.min(c.w / 2).min(c.h / 2));
    let reach = i64::from(MAX_SPREAD) + r;
    let dx = (left + r - x).max(x - (left + w - 1 - r)).clamp(0, reach);
    let dy = (top + r - y).max(y - (top + h - 1 - r)).clamp(0, reach);
    let length16 = i64::from(isqrt((256 * (dx * dx + dy * dy)) as u32));
    (length16 - 16 * r).max(0)
}
