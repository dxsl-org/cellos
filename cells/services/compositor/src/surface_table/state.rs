use api::display::{PixelFormat, Rect, SurfaceRole};

use super::{configure::PendingConfigure, lifecycle::PendingClose};

/// Pixel data source for a surface.
pub(super) enum PixelSource {
    /// App Cell's Grant buffer — compositor reads directly via a read-only pointer.
    Grant { ptr: *const u8, reg_id: usize },
    /// Compositor-owned fallback buffer (legacy `WRITE_PIXELS` path).
    Owned(alloc::boxed::Box<[u8]>),
}

// SAFETY: compositor runs as a single cooperative task; no other task touches
// PixelSource concurrently. The Grant pointer is stable until its owner detaches it.
unsafe impl Send for PixelSource {}

/// State for one live surface.
pub struct SurfaceState {
    /// Screen position.
    pub x: i32,
    pub y: i32,
    /// Dimensions in pixels.
    pub w: u32,
    pub h: u32,
    /// Pixel format (default: Bgra8888).
    pub fmt: PixelFormat,
    pub(super) source: PixelSource,
    /// Accumulated damage since last flush. `None` = no damage.
    pub damage: Option<Rect>,
    /// TID of the cell that created this surface (input routing + ownership checks).
    pub owner: usize,
    /// Whether the compositor may select this surface for desktop input.
    pub role: SurfaceRole,
    /// UTF-8 window title, kept separate from client pixel storage.
    title: alloc::string::String,
    pub(super) state: api::display::WindowState,
    pub(super) normal_rect: Rect,
    pub(super) minimized_from: api::display::WindowState,
    pub(super) pending_configure: Option<PendingConfigure>,
    pub(super) retired_grant_id: Option<usize>,
    pub(super) pending_close: Option<PendingClose>,
    pub(super) serial: u32,
}

impl SurfaceState {
    /// Allocate a new surface with a compositor-owned pixel buffer.
    pub fn new(x: i32, y: i32, w: u32, h: u32, owner: usize, role: SurfaceRole) -> Self {
        let rect = Rect { x, y, w, h };
        Self {
            x,
            y,
            w,
            h,
            fmt: PixelFormat::Bgra8888,
            source: PixelSource::Owned(alloc::vec::Vec::new().into_boxed_slice()),
            damage: None,
            owner,
            role,
            title: alloc::string::String::new(),
            state: api::display::WindowState::Normal,
            minimized_from: api::display::WindowState::Normal,
            normal_rect: rect,
            pending_configure: None,
            retired_grant_id: None,
            pending_close: None,
            serial: 0,
        }
    }

    /// Replace the title after the caller has validated the fixed protocol frame.
    pub fn set_title(&mut self, title: &str) {
        self.title.clear();
        self.title.push_str(title);
    }

    /// Current compositor-managed state.
    pub const fn state(&self) -> api::display::WindowState {
        self.state
    }

    /// True only while content may participate in scanout.
    pub const fn is_visible_for_paint(&self) -> bool {
        matches!(
            self.state,
            api::display::WindowState::Normal | api::display::WindowState::Maximized
        )
    }

    /// True only for a visible interactive surface.
    pub fn accepts_input(&self) -> bool {
        self.role == SurfaceRole::Interactive && self.is_visible_for_paint()
    }

    /// Background surfaces are deliberately outside the window-manager state machine.
    pub fn is_window_managed(&self) -> bool {
        self.role == SurfaceRole::Interactive
    }

    /// Move a normal surface and retain that position for a later restore.
    pub fn move_to(&mut self, x: i32, y: i32) {
        self.x = x;
        self.y = y;
        if self.state == api::display::WindowState::Normal {
            self.normal_rect.x = x;
            self.normal_rect.y = y;
        }
    }

    /// Clear the damage accumulator after a flush.
    pub fn clear_damage(&mut self) {
        self.damage = None;
    }

    /// Bounding rect of this surface on screen.
    pub fn screen_rect(&self) -> Rect {
        Rect {
            x: self.x,
            y: self.y,
            w: self.w,
            h: self.h,
        }
    }
}
