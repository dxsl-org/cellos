use api::display::{PixelFormat, Rect};

use super::{state::PixelSource, SurfaceState};

/// Largest compositor-owned buffer a legacy `WRITE_PIXELS` client may cause the
/// compositor to allocate. The compositor heap is 10 MiB and also holds the
/// scanout staging buffer, so a client-sized allocation must stay well below it.
const MAX_OWNED_PIXEL_BYTES: usize = 1024 * 1024;

impl SurfaceState {
    /// Attach a Grant buffer from the app cell.
    ///
    /// A legacy attachment activates immediately. When a configure is pending, only
    /// a Grant matching its proposed dimensions is staged; the active source remains
    /// untouched until the owner acknowledges that proposal.
    pub fn attach_grant(&mut self, reg_id: usize, w: u32, h: u32, fmt: PixelFormat) -> bool {
        let source = PixelSource::Grant { reg_id };
        if let Some(pending) = &mut self.pending_configure {
            if pending.rect.w != w || pending.rect.h != h {
                return false;
            }
            pending.staged_source = Some(source);
            pending.staged_format = Some(fmt);
            return true;
        }
        if self.retired_grant_id.is_some() {
            return false;
        }
        self.w = w;
        self.h = h;
        self.fmt = fmt;
        self.source = source;
        if self.state == api::display::WindowState::Normal {
            self.normal_rect = self.screen_rect();
        }
        true
    }

    /// Detach a staged Grant first, keeping the active mapping live during resize.
    pub fn detach_grant(&mut self) -> bool {
        if let Some(pending) = &mut self.pending_configure {
            if pending.staged_source.take().is_some() {
                pending.staged_format = None;
                return true;
            }
        }
        if self.retired_grant_id.is_some() {
            return false;
        }
        self.source = PixelSource::Owned(alloc::vec::Vec::new().into_boxed_slice());
        true
    }

    /// Registered Grant IDs retained by this surface, including uncommitted and
    /// retired replacement mappings.
    pub fn grant_ids(&self) -> [Option<usize>; 3] {
        let active = match &self.source {
            PixelSource::Grant { reg_id } => Some(*reg_id),
            PixelSource::Owned(_) => None,
        };
        let staged = self.pending_configure.as_ref().and_then(|pending| {
            match pending.staged_source.as_ref() {
                Some(PixelSource::Grant { reg_id }) => Some(*reg_id),
                _ => None,
            }
        });
        [active, staged, self.retired_grant_id]
    }

    /// Acknowledge that the owner will release exactly the retired replacement
    /// mapping. The active Grant is never affected by this operation.
    pub fn detach_replaced_grant(&mut self, reg_id: usize) -> bool {
        if self.retired_grant_id == Some(reg_id) {
            self.retired_grant_id = None;
            true
        } else {
            false
        }
    }

    /// Bytes the active presentation needs, or `None` when the geometry cannot be
    /// represented (never true for geometry admitted by the table).
    fn content_len(&self) -> Option<usize> {
        (self.w as usize)
            .checked_mul(self.h as usize)?
            .checked_mul(self.fmt.bpp() as usize)
    }

    /// Pixel data for one blit.
    ///
    /// A Grant is resolved through `resolve` (the `GrantSlice` syscall in
    /// production) on every call and the pointer is used only for this blit. A
    /// Grant the owner has released, or that now resolves smaller than the surface
    /// needs, is detached: the surface paints nothing until the owner attaches a
    /// live buffer again.
    ///
    /// Residual: the owner can still free the Grant between this resolution and
    /// the copy, because a non-VFS `GrantSlice` installs no pin in the kernel.
    pub fn resolve_pixels(
        &mut self,
        resolve: impl FnOnce(usize) -> Option<(*mut u8, usize)>,
    ) -> &[u8] {
        if let PixelSource::Grant { reg_id } = self.source {
            let live = self.content_len().and_then(|needed| {
                resolve(reg_id)
                    .filter(|&(ptr, len)| !ptr.is_null() && len >= needed)
                    .map(|(ptr, _)| (ptr, needed))
            });
            match live {
                // SAFETY: the kernel just resolved `reg_id` for this task to a
                // mapping of at least `needed` bytes; the slice is consumed by
                // the caller's blit before the next IPC is served.
                Some((ptr, needed)) => {
                    return unsafe { core::slice::from_raw_parts(ptr.cast_const(), needed) }
                }
                None => self.source = PixelSource::Owned(alloc::vec::Vec::new().into_boxed_slice()),
            }
        }
        match &self.source {
            PixelSource::Owned(buf) => buf,
            PixelSource::Grant { .. } => &[],
        }
    }

    /// Write pixel data into an Owned surface (legacy `WRITE_PIXELS` path).
    ///
    /// The rectangle must lie inside the surface and the payload must hold all of
    /// it; anything else is dropped, so a client cannot drive slice arithmetic
    /// out of bounds or make the compositor allocate past its owned-buffer cap.
    pub fn write_pixels(&mut self, px: i32, py: i32, pw: u32, ph: u32, data: &[u8]) {
        let (Ok(x), Ok(y)) = (u32::try_from(px), u32::try_from(py)) else {
            return;
        };
        let fits = |start: u32, len: u32, bound: u32| {
            start.checked_add(len).is_some_and(|end| end <= bound)
        };
        if !fits(x, pw, self.w) || !fits(y, ph, self.h) {
            return;
        }
        let row_bytes = pw as usize * 4;
        if row_bytes
            .checked_mul(ph as usize)
            .is_none_or(|expected| data.len() < expected)
        {
            return;
        }
        let Some(needed) = (self.w as usize)
            .checked_mul(self.h as usize)
            .and_then(|pixels| pixels.checked_mul(4))
            .filter(|&needed| needed <= MAX_OWNED_PIXEL_BYTES)
        else {
            return;
        };
        if let PixelSource::Owned(b) = &self.source {
            if b.len() < needed {
                let mut fresh = alloc::vec::Vec::new();
                if fresh.try_reserve_exact(needed).is_err() {
                    return;
                }
                fresh.resize(needed, 0);
                self.source = PixelSource::Owned(fresh.into_boxed_slice());
            }
        }
        let stride = self.w as usize * 4;
        let buf = match &mut self.source {
            PixelSource::Owned(b) => b,
            PixelSource::Grant { .. } => return,
        };
        // In bounds: x + pw <= w and y + ph <= h, so every row ends at or before
        // `needed`, and `data` holds `row_bytes * ph` bytes.
        for row in 0..ph as usize {
            let dst = (y as usize + row) * stride + x as usize * 4;
            let src = row * row_bytes;
            buf[dst..dst + row_bytes].copy_from_slice(&data[src..src + row_bytes]);
        }
        let new_dmg = Rect {
            x: px,
            y: py,
            w: pw,
            h: ph,
        };
        self.damage = Some(match self.damage {
            Some(existing) => existing.union(&new_dmg),
            None => new_dmg,
        });
    }
}

#[cfg(test)]
#[path = "pixels_tests.rs"]
mod tests;
