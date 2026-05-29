//! Software rasterizer — blends damaged surfaces into the screen framebuffer
//! and flushes the dirty region to the VirtIO GPU via the `GpuFlush` syscall.

extern crate alloc;

use alloc::vec;
use api::display::{Rect, FALLBACK_WIDTH, FALLBACK_HEIGHT};
use ostd::syscall::sys_gpu_flush;
use crate::surface_table::SurfaceTable;
use crate::z_order::ZOrder;

/// Screen framebuffer owned by the compositor (BGRA8888).
pub struct ScreenFb {
    pixels: alloc::vec::Vec<u8>,
    pub width:  u32,
    pub height: u32,
}

impl ScreenFb {
    /// Allocate a zeroed framebuffer of the given dimensions.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            pixels: vec![0u8; (width * height * 4) as usize],
            width,
            height,
        }
    }

    /// Blit one surface's pixels into the screen FB at its screen position.
    ///
    /// Clips to the screen boundary; surfaces that are partially off-screen
    /// are rendered up to the edge.
    fn blit_surface(&mut self, s: &crate::surface_table::SurfaceState) {
        let sx = s.x.max(0) as u32;
        let sy = s.y.max(0) as u32;
        let clip_x = (-s.x).max(0) as u32; // surface offset if partially off-screen
        let clip_y = (-s.y).max(0) as u32;
        let w = (s.w.saturating_sub(clip_x)).min(self.width.saturating_sub(sx));
        let h = (s.h.saturating_sub(clip_y)).min(self.height.saturating_sub(sy));
        if w == 0 || h == 0 { return; }

        let screen_stride = self.width as usize * 4;
        let surf_stride   = s.w as usize * 4;

        for row in 0..h as usize {
            let dst_off = (sy as usize + row) * screen_stride + sx as usize * 4;
            let src_off = (clip_y as usize + row) * surf_stride + clip_x as usize * 4;
            let n = w as usize * 4;
            if dst_off + n <= self.pixels.len() && src_off + n <= s.pixels.len() {
                self.pixels[dst_off..dst_off + n]
                    .copy_from_slice(&s.pixels[src_off..src_off + n]);
            }
        }
    }

    /// Flush `dirty_rect` from the screen FB to the GPU.
    ///
    /// Clamps the dirty rect to the screen boundary before calling the kernel.
    fn flush_rect(&self, dirty: Rect) {
        let x = dirty.x.max(0) as u32;
        let y = dirty.y.max(0) as u32;
        let w = dirty.w.min(self.width.saturating_sub(x));
        let h = dirty.h.min(self.height.saturating_sub(y));
        if w == 0 || h == 0 { return; }

        // Build a sub-rect pixel buffer to send to the kernel.
        let stride = self.width as usize * 4;
        let mut sub = alloc::vec![0u8; (w * h * 4) as usize];
        for row in 0..h as usize {
            let src = (y as usize + row) * stride + x as usize * 4;
            let dst = row * w as usize * 4;
            let n   = w as usize * 4;
            if src + n <= self.pixels.len() {
                sub[dst..dst + n].copy_from_slice(&self.pixels[src..src + n]);
            }
        }
        let _ = sys_gpu_flush(&sub, x, y, w, h);
    }
}

/// Render one frame: blit all damaged surfaces then flush the combined dirty rect.
///
/// Returns the dirty rect that was flushed, or `None` if nothing was dirty.
pub fn render_frame(
    fb: &mut ScreenFb,
    table: &mut SurfaceTable,
    z_order: &ZOrder,
) -> Option<Rect> {
    // Collect the union of all surface dirty rects.
    let mut dirty: Option<Rect> = None;
    for cap in z_order.iter_bottom_to_top() {
        if let Some(s) = table.get(cap) {
            if let Some(dmg) = s.damage {
                // Translate surface-local damage to screen coordinates.
                let screen_dmg = Rect {
                    x: s.x + dmg.x,
                    y: s.y + dmg.y,
                    w: dmg.w,
                    h: dmg.h,
                };
                dirty = Some(match dirty {
                    Some(acc) => acc.union(&screen_dmg),
                    None => screen_dmg,
                });
            }
        }
    }

    let dirty = dirty?;

    // Re-blit all surfaces that overlap the dirty rect (bottom to top).
    for cap in z_order.iter_bottom_to_top() {
        if let Some(s) = table.get(cap) {
            if s.screen_rect().intersects(&dirty) {
                fb.blit_surface(s);
            }
        }
    }

    // Clear damage on all surfaces.
    for cap in z_order.iter_bottom_to_top() {
        if let Some(s) = table.get_mut(cap) {
            s.clear_damage();
        }
    }

    // Flush the dirty rect to the GPU.
    fb.flush_rect(dirty);
    Some(dirty)
}

/// Return the default screen dimensions (probed from GPU at startup).
pub fn default_screen_size() -> (u32, u32) {
    (FALLBACK_WIDTH, FALLBACK_HEIGHT)
}
