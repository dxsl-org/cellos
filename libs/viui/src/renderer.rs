// SPDX-License-Identifier: MIT
//! ViRenderer trait — abstract rendering backend for ViUI v2.
//!
//! # Backend selection
//!
//! G1: `FramebufferRenderer` (CPU software rasterizer, default)
//! G2+: GPU backend implementing the same trait; widget code is unchanged.
//!
//! # Object safety
//!
//! `ViRenderer` is object-safe; use `Box<dyn ViRenderer>` to store a
//! heap-allocated renderer when the concrete type is unknown at compile time.
//!
//! # Lifetime note
//!
//! `FramebufferCanvas<'fb>` borrows pixels from `ViSurface`. The closure pattern
//! in `render()` confines that borrow to the stack frame, avoiding any
//! self-referential struct or lifetime gymnastics.

extern crate alloc;

use alloc::rc::Rc;
use api::display::Rect as SurfaceRect;
use core::cell::RefCell;

use crate::canvas::{FramebufferCanvas, ViCanvas};
use crate::layout::Rect;
use ostd::display::ViSurface;

// ─── ViRenderer ──────────────────────────────────────────────────────────────

/// Abstract rendering backend.
///
/// # Contract
///
/// - Call `render()` once per frame, after collecting dirty rects via `DirtyRect`.
/// - All painting must happen inside the `draw` closure — canvas is invalid after return.
/// - `damage` is advisory to drawing but is submitted precisely by compatible backends.
pub trait ViRenderer {
    /// Run a paint closure with exclusive canvas access, then submit the frame.
    ///
    /// `damage`: the screen region that changed this frame. `None` = full surface.
    fn render(&mut self, damage: Option<Rect>, draw: &mut dyn FnMut(&mut dyn ViCanvas));

    /// Surface dimensions in pixels as `(width, height)`.
    fn size(&self) -> (u32, u32);
}

// ─── FramebufferRenderer ─────────────────────────────────────────────────────

/// G1 CPU renderer wrapping a `ViSurface` + `FramebufferCanvas`.
///
/// The `FramebufferCanvas<'fb>` borrow is confined to the `render()` call
/// stack frame — no heap allocation, no unsafe, no self-referential struct.
pub struct FramebufferRenderer {
    surf: SurfaceHandle,
}

/// Single-threaded access to a Grant-backed compositor surface.
///
/// The handle lets a [`FramebufferRenderer`] and a managed lifecycle runner
/// operate on the same surface without duplicating ownership of its Grant.
pub(crate) type SurfaceHandle = Rc<RefCell<ViSurface>>;

/// Wrap a surface for sharing between ViUI's renderer and lifecycle adapter.
pub(crate) fn shared_surface(surf: ViSurface) -> SurfaceHandle {
    Rc::new(RefCell::new(surf))
}

impl FramebufferRenderer {
    pub fn new(surf: ViSurface) -> Self {
        Self::from_surface_handle(shared_surface(surf))
    }

    /// Create a renderer for an existing managed-surface handle.
    pub(crate) fn from_surface_handle(surf: SurfaceHandle) -> Self {
        Self { surf }
    }

    /// Unwrap the inner `ViSurface` (e.g. for IPC cleanup after app exit).
    ///
    /// # Panics
    /// Panics when another lifecycle or renderer handle is still alive.
    pub fn into_surf(self) -> ViSurface {
        match Rc::try_unwrap(self.surf) {
            Ok(surface) => surface.into_inner(),
            Err(_) => panic!("FramebufferRenderer surface is still shared"),
        }
    }
}

impl ViRenderer for FramebufferRenderer {
    fn render(&mut self, damage: Option<Rect>, draw: &mut dyn FnMut(&mut dyn ViCanvas)) {
        let mut surf = self.surf.borrow_mut();
        let (w, h) = (surf.width(), surf.height());
        {
            let stride = surf.stride() as u32;
            let pixels = surf.pixels_mut();
            let mut canvas = FramebufferCanvas::new(pixels, stride, w, h);
            draw(&mut canvas);
        }
        match damage {
            None => surf.damage_all(),
            Some(damage) => {
                if let Some(rect) = clipped_damage(damage, w, h) {
                    surf.damage(rect);
                }
            }
        }
    }

    fn size(&self) -> (u32, u32) {
        let surf = self.surf.borrow();
        (surf.width(), surf.height())
    }
}

/// Convert ViUI float bounds into a clipped, outward-rounded surface rectangle.
fn clipped_damage(damage: Rect, width: u32, height: u32) -> Option<SurfaceRect> {
    if damage.w <= 0.0
        || damage.h <= 0.0
        || !damage.x.is_finite()
        || !damage.y.is_finite()
        || !damage.w.is_finite()
        || !damage.h.is_finite()
    {
        return None;
    }
    let right = damage.x + damage.w;
    let bottom = damage.y + damage.h;
    if !right.is_finite() || !bottom.is_finite() {
        return None;
    }
    let left = damage.x.max(0.0).min(width as f32);
    let top = damage.y.max(0.0).min(height as f32);
    let right = right.max(0.0).min(width as f32);
    let bottom = bottom.max(0.0).min(height as f32);
    let x = floor_nonnegative(left);
    let y = floor_nonnegative(top);
    let right = ceil_nonnegative(right);
    let bottom = ceil_nonnegative(bottom);
    (right > x && bottom > y).then_some(SurfaceRect {
        x: x as i32,
        y: y as i32,
        w: right - x,
        h: bottom - y,
    })
}

/// Convert a bounded non-negative finite float to its integer floor in `no_std`.
fn floor_nonnegative(value: f32) -> u32 {
    value as u32
}

/// Convert a bounded non-negative finite float to its integer ceiling in `no_std`.
fn ceil_nonnegative(value: f32) -> u32 {
    let truncated = floor_nonnegative(value);
    truncated.saturating_add(u32::from((truncated as f32) < value))
}

#[cfg(test)]
mod tests {
    use super::clipped_damage;
    use crate::layout::Rect;

    #[test]
    fn damage_rounds_outward() {
        let rect = clipped_damage(Rect::new(1.2, 2.8, 3.1, 4.01), 20, 20).unwrap();
        assert_eq!((rect.x, rect.y, rect.w, rect.h), (1, 2, 4, 5));
    }

    #[test]
    fn damage_clips_to_surface() {
        let rect = clipped_damage(Rect::new(-3.5, 8.2, 7.1, 4.1), 10, 10).unwrap();
        assert_eq!((rect.x, rect.y, rect.w, rect.h), (0, 8, 4, 2));
    }

    #[test]
    fn damage_ignores_empty_offscreen_and_non_finite_rects() {
        assert!(clipped_damage(Rect::new(1.0, 1.0, 0.0, 2.0), 10, 10).is_none());
        assert!(clipped_damage(Rect::new(12.0, 1.0, 2.0, 2.0), 10, 10).is_none());
        assert!(clipped_damage(Rect::new(f32::NAN, 1.0, 2.0, 2.0), 10, 10).is_none());
    }
}
