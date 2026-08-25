use api::display::{PixelFormat, Rect};

use super::{state::PixelSource, SurfaceState};

impl SurfaceState {
    /// Attach a Grant buffer from the app cell.
    ///
    /// A legacy attachment activates immediately. When a configure is pending, only
    /// a Grant matching its proposed dimensions is staged; the active source remains
    /// untouched until the owner acknowledges that proposal.
    pub fn attach_grant(
        &mut self,
        ptr: *const u8,
        reg_id: usize,
        w: u32,
        h: u32,
        fmt: PixelFormat,
    ) -> bool {
        let source = PixelSource::Grant { ptr, reg_id };
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
            PixelSource::Grant { reg_id, .. } => Some(*reg_id),
            PixelSource::Owned(_) => None,
        };
        let staged = self.pending_configure.as_ref().and_then(|pending| {
            match pending.staged_source.as_ref() {
                Some(PixelSource::Grant { reg_id, .. }) => Some(*reg_id),
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

    /// Read access to pixel data — either from the Grant or the Owned buffer.
    pub fn pixels(&self) -> &[u8] {
        match &self.source {
            PixelSource::Grant { ptr, .. } => {
                let len = (self.w * self.h * self.fmt.bpp()) as usize;
                // SAFETY: ownership and detach protocol keep the grant live while active.
                unsafe { core::slice::from_raw_parts(*ptr, len) }
            }
            PixelSource::Owned(buf) => buf,
        }
    }

    /// Write pixel data into an Owned surface (legacy `WRITE_PIXELS` path).
    pub fn write_pixels(&mut self, px: i32, py: i32, pw: u32, ph: u32, data: &[u8]) {
        let needed = (self.w * self.h * 4) as usize;
        if let PixelSource::Owned(b) = &self.source {
            if b.len() < needed {
                self.source = PixelSource::Owned(alloc::vec![0u8; needed].into_boxed_slice());
            }
        }
        let buf = match &mut self.source {
            PixelSource::Owned(b) => b,
            PixelSource::Grant { .. } => return,
        };
        let expected = (pw * ph * 4) as usize;
        if data.len() < expected {
            return;
        }
        let stride = self.w as usize * 4;
        for row in 0..ph as usize {
            let dst_off = (py as usize + row) * stride + px as usize * 4;
            let src_off = row * pw as usize * 4;
            let row_bytes = pw as usize * 4;
            if dst_off + row_bytes <= buf.len() {
                buf[dst_off..dst_off + row_bytes]
                    .copy_from_slice(&data[src_off..src_off + row_bytes]);
            }
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
