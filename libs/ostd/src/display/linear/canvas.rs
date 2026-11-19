//! Clipped BGRA canvas: spans, rectangles, rounded rectangles and 1px lines.

use super::{Bgra, Radii, RoundRect};

/// Clipped BGRA drawing target over a caller-owned pixel buffer.
pub struct Canvas<'a> {
    pixels: &'a mut [u8],
    stride: usize,
    pub(super) x0: i64,
    pub(super) y0: i64,
    pub(super) x1: i64,
    pub(super) y1: i64,
}

impl<'a> Canvas<'a> {
    /// `stride` is in bytes. Rows or columns that do not fit `pixels` are clipped.
    pub fn new(pixels: &'a mut [u8], width: u32, height: u32, stride: usize) -> Self {
        let width = u64::from(width).min(stride as u64 / 4);
        let row_bytes = width as usize * 4;
        let rows = if width == 0 || pixels.len() < row_bytes {
            0
        } else {
            (pixels.len() - row_bytes) / stride + 1
        };
        let height = u64::from(height).min(rows as u64);
        Self {
            pixels,
            stride,
            x0: 0,
            y0: 0,
            x1: width as i64,
            y1: height as i64,
        }
    }

    /// Restrict further drawing to the intersection with `(x, y, w, h)`.
    pub fn clip_to(mut self, x: i32, y: i32, w: u32, h: u32) -> Self {
        self.x0 = self.x0.max(i64::from(x));
        self.y0 = self.y0.max(i64::from(y));
        self.x1 = self.x1.min(i64::from(x) + i64::from(w));
        self.y1 = self.y1.min(i64::from(y) + i64::from(h));
        self
    }

    pub fn fill_rect(&mut self, x: i32, y: i32, w: u32, h: u32, color: Bgra) {
        self.fill_round(&RoundRect::new(x, y, w, h, Radii::default()), color);
    }

    pub fn stroke_rect(&mut self, x: i32, y: i32, w: u32, h: u32, color: Bgra) {
        self.stroke_round(&RoundRect::new(x, y, w, h, Radii::default()), color);
    }

    pub fn hline(&mut self, x: i32, y: i32, len: u32, color: Bgra) {
        let x = i64::from(x);
        self.span(i64::from(y), x, x + i64::from(len), color);
    }

    pub fn vline(&mut self, x: i32, y: i32, len: u32, color: Bgra) {
        for row in 0..i64::from(len) {
            self.pixel(i64::from(x), i64::from(y) + row, color);
        }
    }

    /// 1px Bresenham segment, both endpoints inclusive.
    pub fn line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: Bgra) {
        let (mut x, mut y) = (i64::from(x0), i64::from(y0));
        let (x1, y1) = (i64::from(x1), i64::from(y1));
        let (dx, dy) = ((x1 - x).abs(), -(y1 - y).abs());
        let (sx, sy) = (if x < x1 { 1 } else { -1 }, if y < y1 { 1 } else { -1 });
        let mut err = dx + dy;
        loop {
            self.pixel(x, y, color);
            if x == x1 && y == y1 {
                return;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    }

    pub fn fill_round(&mut self, shape: &RoundRect, color: Bgra) {
        for row in self.rows(shape) {
            let (li, ri) = shape.insets(row);
            let x = i64::from(shape.x);
            let y = i64::from(shape.y) + i64::from(row);
            self.span(
                y,
                x + i64::from(li),
                x + i64::from(shape.w) - i64::from(ri),
                color,
            );
        }
    }

    /// 1px outline covering exactly the boundary pixels of [`Self::fill_round`].
    pub fn stroke_round(&mut self, shape: &RoundRect, color: Bgra) {
        for row in self.rows(shape) {
            let ((l0, l1), (r0, r1)) = shape.outline(row);
            let x = i64::from(shape.x);
            let y = i64::from(shape.y) + i64::from(row);
            self.span(y, x + i64::from(l0), x + i64::from(l1), color);
            self.span(y, x + i64::from(r0), x + i64::from(r1), color);
        }
    }

    /// Shape rows that intersect the clip, as row indices relative to `shape.y`.
    fn rows(&self, shape: &RoundRect) -> core::ops::Range<u32> {
        let top = i64::from(shape.y);
        let first = (self.y0 - top).clamp(0, i64::from(shape.h));
        let last = (self.y1 - top).clamp(first, i64::from(shape.h));
        first as u32..last as u32
    }

    pub(super) fn pixel(&mut self, x: i64, y: i64, color: Bgra) {
        self.span(y, x, x + 1, color);
    }

    pub(super) fn span(&mut self, y: i64, x0: i64, x1: i64, color: Bgra) {
        let (x0, x1) = (x0.max(self.x0), x1.min(self.x1));
        if y < self.y0 || y >= self.y1 || x0 >= x1 || color[3] == 0 {
            return;
        }
        let start = y as usize * self.stride + x0 as usize * 4;
        let row = &mut self.pixels[start..start + (x1 - x0) as usize * 4];
        if color[3] == 255 {
            for px in row.chunks_exact_mut(4) {
                px.copy_from_slice(&color);
            }
            return;
        }
        let a = u32::from(color[3]);
        for px in row.chunks_exact_mut(4) {
            for ch in 0..3 {
                px[ch] =
                    ((u32::from(color[ch]) * a + u32::from(px[ch]) * (255 - a) + 127) / 255) as u8;
            }
            px[3] = 255;
        }
    }
}
