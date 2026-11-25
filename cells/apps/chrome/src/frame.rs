//! The guest scanout Grant and copies from it into the window surface.

use ostd::syscall::{sys_grant_copy_rows_owned_by, GrantRows};
use wire::display::Rect;

const BYTES_PER_PIXEL: usize = 4;
/// Rows read, spread over the frame, when asking whether the guest has drawn.
const SAMPLED_ROWS: usize = 64;

/// The hypervisor's scanout Grant: `width`x`height` BGRA8888, rows packed
/// (`width * 4` bytes), shared read-only and owned by `owner_tid`.
pub struct GuestFrame {
    grant: usize,
    owner_tid: usize,
    width: u32,
    height: u32,
}

impl GuestFrame {
    pub const fn new(grant: u32, owner_tid: usize, width: u32, height: u32) -> Self {
        Self {
            grant: grant as usize,
            owner_tid,
            width,
            height,
        }
    }

    /// The whole frame.
    pub const fn bounds(&self) -> Rect {
        Rect {
            x: 0,
            y: 0,
            w: self.width,
            h: self.height,
        }
    }

    /// Whether the guest has drawn anything: a new scanout Grant is zeroed, and
    /// cage's output is black until its application's first frame. `false` too
    /// when the Grant is no longer readable.
    pub fn has_content(&self) -> bool {
        let stride = self.width as usize * BYTES_PER_PIXEL;
        let step = (self.height as usize).div_ceil(SAMPLED_ROWS).max(1);
        let mut row = alloc::vec![0u8; stride];
        (0..self.height as usize).step_by(step).any(|y| {
            let rows = GrantRows {
                src_offset: y * stride,
                src_stride: stride,
                dst_offset: 0,
                dst_stride: stride,
                row_len: stride,
                rows: 1,
            };
            sys_grant_copy_rows_owned_by(self.grant, self.owner_tid, rows, &mut row).is_some()
                && has_colour(&row)
        })
    }

    /// Copy guest rectangle `rect` to the same position in `dst` (a surface of
    /// `dst_width`x`dst_height` pixels, `dst_stride` bytes per row). Returns the
    /// rectangle actually copied, for damage; `None` if nothing overlapped or
    /// the Grant is no longer readable.
    pub fn copy_to(
        &self,
        rect: Rect,
        dst: &mut [u8],
        dst_stride: usize,
        dst_width: u32,
        dst_height: u32,
    ) -> Option<Rect> {
        let clipped = clip(rect, self.width.min(dst_width), self.height.min(dst_height))?;
        let (x, y) = (clipped.x as usize, clipped.y as usize);
        let src_stride = self.width as usize * BYTES_PER_PIXEL;
        let rows = GrantRows {
            src_offset: y * src_stride + x * BYTES_PER_PIXEL,
            src_stride,
            dst_offset: y * dst_stride + x * BYTES_PER_PIXEL,
            dst_stride,
            row_len: clipped.w as usize * BYTES_PER_PIXEL,
            rows: clipped.h as usize,
        };
        sys_grant_copy_rows_owned_by(self.grant, self.owner_tid, rows, dst)?;
        Some(clipped)
    }
}

/// Whether any BGRX pixel of `pixels` is not black. The fourth byte is alpha or
/// padding, set or clear depending on who filled the frame, and is not colour.
fn has_colour(pixels: &[u8]) -> bool {
    pixels
        .as_chunks::<BYTES_PER_PIXEL>()
        .0
        .iter()
        .any(|pixel| pixel[..3] != [0, 0, 0])
}

/// `rect` clipped to `0..width` x `0..height`; `None` if nothing remains.
fn clip(rect: Rect, width: u32, height: u32) -> Option<Rect> {
    let span = |start: i32, len: u32, limit: u32| {
        let end = (i64::from(start) + i64::from(len)).min(i64::from(limit));
        let start = i64::from(start).max(0);
        (start < end).then_some((start as u32, end as u32))
    };
    let (x0, x1) = span(rect.x, rect.w, width)?;
    let (y0, y1) = span(rect.y, rect.h, height)?;
    Some(Rect {
        x: x0 as i32,
        y: y0 as i32,
        w: x1 - x0,
        h: y1 - y0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rectangles_are_clipped_to_the_shared_area() {
        let rect = |x, y, w, h| Rect { x, y, w, h };
        assert_eq!(clip(rect(1, 1, 10, 10), 3, 3), Some(rect(1, 1, 2, 2)));
        assert_eq!(clip(rect(-4, 2, 6, 1), 3, 3), Some(rect(0, 2, 2, 1)));
        assert_eq!(clip(rect(-4, -4, 2, 2), 3, 3), None, "entirely above-left");
        assert_eq!(
            clip(rect(3, 0, 5, 1), 3, 3),
            None,
            "starts past the right edge"
        );
        assert_eq!(clip(rect(0, 0, 0, 5), 3, 3), None, "empty");
    }

    #[test]
    fn a_black_frame_is_blank_whatever_its_alpha() {
        assert!(
            !has_colour(&[0, 0, 0, 0, 0, 0, 0, 255]),
            "cleared and opaque black"
        );
        assert!(
            has_colour(&[0, 0, 0, 255, 0, 1, 0, 0]),
            "one coloured pixel"
        );
    }
}
