//! The guest scanout Grant and copies from it into the window surface.

use ostd::syscall::{sys_grant_copy_rows_owned_by, GrantRows};
use wire::display::Rect;

const BYTES_PER_PIXEL: usize = 4;

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
}
