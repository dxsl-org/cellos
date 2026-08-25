//! Scanout framebuffer storage, raster operations, and GPU flush.

extern crate alloc;

use alloc::vec;
use api::display::Rect;
use ostd::syscall::{sys_get_resolution, sys_gpu_flush};

use crate::cursor_sprite::{cursor_pixel, CURSOR_H, CURSOR_W};
use crate::surface_table::SurfaceState;
use crate::window_decoration;

pub struct ScreenFb {
    pixels: alloc::vec::Vec<u8>,
    staging: alloc::vec::Vec<u8>,
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
            width,
            height,
        }
    }

    pub fn blit_surface(&mut self, s: &SurfaceState) {
        let sx = s.x.max(0) as u32;
        let sy = s.y.max(0) as u32;
        let clip_x = (-s.x).max(0) as u32;
        let clip_y = (-s.y).max(0) as u32;
        let w = (s.w.saturating_sub(clip_x)).min(self.width.saturating_sub(sx));
        let h = (s.h.saturating_sub(clip_y)).min(self.height.saturating_sub(sy));
        let screen_stride = self.width as usize * 4;
        let surf_stride = s.w as usize * 4;
        let src_pixels = s.pixels();
        for row in 0..h as usize {
            let dst = (sy as usize + row) * screen_stride + sx as usize * 4;
            let src = (clip_y as usize + row) * surf_stride + clip_x as usize * 4;
            let len = w as usize * 4;
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

    pub fn paint_window_decoration(&mut self, surface: Rect, dirty: Rect, active: bool) {
        window_decoration::paint(
            &mut self.pixels,
            self.width,
            self.height,
            surface,
            dirty,
            active,
        );
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
