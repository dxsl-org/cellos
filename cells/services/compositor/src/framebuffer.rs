//! Scanout framebuffer storage, raster operations, and GPU flush.

extern crate alloc;

use alloc::vec;
use api::display::Rect;
use core::sync::atomic::{AtomicBool, Ordering};
use ostd::display::linear::{Bgra, Canvas, RoundRect, Shadow};
use ostd::syscall::{sys_get_resolution, sys_gpu_flush};

use crate::cursor_sprite::{cursor_pixel, CURSOR_H, CURSOR_W};
use crate::surface_table::SurfaceState;
use crate::window_decoration;

pub struct ScreenFb {
    pixels: alloc::vec::Vec<u8>,
    staging: alloc::vec::Vec<u8>,
    title_font: window_decoration::TitleFont,
    pub width: u32,
    pub height: u32,
}

impl ScreenFb {
    pub fn new(width: u32, height: u32) -> Self {
        assert!(width > 0 && height > 0 && width <= 4096 && height <= 4096);
        let full = (width * height * 4) as usize;
        Self {
            pixels: vec![0; full],
            staging: vec![0; full],
            title_font: window_decoration::TitleFont::new(),
            width,
            height,
        }
    }

    /// Blit only the part of a surface that intersects both `dirty` and scanout.
    ///
    /// Coordinates are intersected in screen space before deriving the matching
    /// source offset, so clipped and negatively-positioned surfaces stay aligned.
    /// A Grant-backed surface is re-resolved here, once per blit.
    /// `row_insets(row)` names the columns of surface row `row` hidden at its
    /// start and end (rounded corners), so what was painted below shows there.
    pub(crate) fn blit_surface_clipped(
        &mut self,
        s: &mut SurfaceState,
        dirty: Rect,
        row_insets: impl Fn(u32) -> (u32, u32),
    ) {
        let surface_left = i64::from(s.x);
        let surface_top = i64::from(s.y);
        let left = surface_left.max(i64::from(dirty.x)).max(0);
        let top = surface_top.max(i64::from(dirty.y)).max(0);
        let right = (surface_left + i64::from(s.w))
            .min(i64::from(dirty.x) + i64::from(dirty.w))
            .min(i64::from(self.width));
        let bottom = (surface_top + i64::from(s.h))
            .min(i64::from(dirty.y) + i64::from(dirty.h))
            .min(i64::from(self.height));
        if left >= right || top >= bottom {
            return;
        }

        let dst_x = left as usize;
        let dst_y = top as usize;
        let src_x = (left - surface_left) as usize;
        let src_y = (top - surface_top) as usize;
        let w = (right - left) as usize;
        let h = (bottom - top) as usize;
        let screen_stride = self.width as usize * 4;
        let surf_stride = s.w as usize * 4;
        let content_w = s.w as usize;
        let src_pixels = s.resolve_pixels(ostd::syscall::sys_grant_slice_with_len_leased_owned_by);
        for row in 0..h {
            let (start, end) = row_insets((src_y + row) as u32);
            let row_x = (start as usize).saturating_sub(src_x);
            let visible_right = content_w
                .saturating_sub(end as usize)
                .saturating_sub(src_x)
                .min(w);
            if row_x >= visible_right {
                continue;
            }
            let row_w = visible_right - row_x;
            let dst = (dst_y + row) * screen_stride + (dst_x + row_x) * 4;
            let src = (src_y + row) * surf_stride + (src_x + row_x) * 4;
            let len = row_w * 4;
            if dst + len <= self.pixels.len() && src + len <= src_pixels.len() {
                self.pixels[dst..dst + len].copy_from_slice(&src_pixels[src..src + len]);
            }
        }
    }

    pub fn clear_rect(&mut self, rect: Rect) {
        let Some(rect) = window_decoration::clip_to_screen(rect, self.width, self.height) else {
            return;
        };
        let stride = self.width as usize * 4;
        for y in rect.y as usize..(rect.y + rect.h as i32) as usize {
            let start = y * stride + rect.x as usize * 4;
            self.pixels[start..start + rect.w as usize * 4].fill(0);
        }
    }

    pub fn paint_window_decoration(
        &mut self,
        surface: Rect,
        dirty: Rect,
        style: &window_decoration::Style<'_>,
    ) {
        window_decoration::paint(
            &mut self.pixels,
            self.width,
            self.height,
            surface,
            dirty,
            style,
            &mut self.title_font,
        );
    }

    /// The window menu `menu` showing `entries` (one per item), inside `dirty`.
    pub fn paint_window_menu(
        &mut self,
        menu: &crate::pointer_router::OpenMenu,
        dirty: Rect,
        entries: &[window_decoration::MenuEntry<'_>],
        palette: &window_decoration::ChromePalette,
    ) {
        window_decoration::paint_menu(
            &mut self.pixels,
            self.width,
            self.height,
            menu.rect,
            dirty,
            &menu.items,
            entries,
            menu.hover,
            palette,
            &mut self.title_font,
        );
    }

    /// A Win+drag's ghost frame and its target's tab strip, as 1px outlines.
    pub fn paint_drag_feedback(
        &mut self,
        drag: &crate::pointer_router::DragFeedback,
        dirty: Rect,
        accent: Bgra,
    ) {
        let mut canvas = Canvas::new(
            &mut self.pixels,
            self.width,
            self.height,
            self.width as usize * 4,
        )
        .clip_to(dirty.x, dirty.y, dirty.w, dirty.h);
        let ghost = drag.ghost;
        canvas.stroke_rect(ghost.x, ghost.y, ghost.w, ghost.h, accent);
        if let Some(target) = drag.target {
            canvas.stroke_rect(
                target.x - 1,
                target.y - 1,
                target.w + 2,
                target.h + 2,
                accent,
            );
            canvas.stroke_rect(
                target.x - 2,
                target.y - 2,
                target.w + 4,
                target.h + 4,
                accent,
            );
        }
    }

    /// Soft shadow around a floating panel, blended over what is painted below.
    pub fn paint_panel_shadow(&mut self, shape: &RoundRect, dirty: Rect, shadow: &Shadow) {
        Canvas::new(
            &mut self.pixels,
            self.width,
            self.height,
            self.width as usize * 4,
        )
        .clip_to(dirty.x, dirty.y, dirty.w, dirty.h)
        .drop_shadow(core::slice::from_ref(shape), shadow);
    }

    pub fn composite_cursor(&mut self, cx: i32, cy: i32, dirty: Rect) {
        let stride = self.width as usize * 4;
        for row in 0..CURSOR_H {
            for col in 0..CURSOR_W {
                let (x, y) = (cx + col as i32, cy + row as i32);
                if x < dirty.x
                    || y < dirty.y
                    || x < 0
                    || y < 0
                    || x >= self.width as i32
                    || y >= self.height as i32
                    || x >= dirty.x + dirty.w as i32
                    || y >= dirty.y + dirty.h as i32
                {
                    continue;
                }
                let Some(src) = cursor_pixel(row, col) else {
                    continue;
                };
                if src[3] == 0 {
                    continue;
                }
                let offset = y as usize * stride + x as usize * 4;
                for (ch, value) in src.iter().take(3).enumerate() {
                    let alpha = src[3] as u32;
                    self.pixels[offset + ch] = ((*value as u32 * alpha
                        + self.pixels[offset + ch] as u32 * (255 - alpha))
                        / 255) as u8;
                }
                self.pixels[offset + 3] = 255;
            }
        }
    }

    pub fn flush_rect(&mut self, dirty: Rect) {
        let Some(rect) = window_decoration::clip_to_screen(dirty, self.width, self.height) else {
            return;
        };
        let stride = self.width as usize * 4;
        let len = rect.w as usize * rect.h as usize * 4;
        for row in 0..rect.h as usize {
            let src = (rect.y as usize + row) * stride + rect.x as usize * 4;
            let dst = row * rect.w as usize * 4;
            self.staging[dst..dst + rect.w as usize * 4]
                .copy_from_slice(&self.pixels[src..src + rect.w as usize * 4]);
        }
        static FIRST_FLUSH: AtomicBool = AtomicBool::new(false);
        if !FIRST_FLUSH.swap(true, Ordering::Relaxed) {
            ostd::io::println("[compositor] first scanout flush submitted");
        }
        let _ = sys_gpu_flush(
            &self.staging[..len],
            rect.x as u32,
            rect.y as u32,
            rect.w,
            rect.h,
        );
    }
}

pub fn default_screen_size() -> (u32, u32) {
    sys_get_resolution()
}

#[cfg(test)]
mod tests;
