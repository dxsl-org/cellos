use wire::display::{PixelFormat, Rect, SurfaceRole};

use super::{configure::PendingConfigure, lifecycle::PendingClose};

/// Pixel data source for a surface.
pub(super) enum PixelSource {
    /// App Cell's Grant buffer, named only by its registration id. No pointer is
    /// cached: the owner may `GrantUnregister` at any time and the compositor
    /// holds no pin, so the mapping is re-resolved for every blit.
    Grant { reg_id: usize },
    /// Compositor-owned fallback buffer (legacy `WRITE_PIXELS` path).
    Owned(alloc::boxed::Box<[u8]>),
}

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
    /// Icon on the Cell Tab (`SET_ICON`); generic until the owner names one.
    icon: ostd::display::linear::icons::LineIcon,
    pub(super) state: wire::display::WindowState,
    pub(super) normal_rect: Rect,
    pub(super) minimized_from: wire::display::WindowState,
    pub(super) pending_configure: Option<PendingConfigure>,
    pub(super) retired_grant_id: Option<usize>,
    pub(super) pending_close: Option<PendingClose>,
    pub(super) serial: u32,
    /// Behind another Cell Tab of its frame: kept `Normal` so it can still be
    /// configured to the frame's size, but neither painted nor given input.
    behind_tab: bool,
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
            icon: ostd::display::linear::icons::LineIcon::Application,
            state: wire::display::WindowState::Normal,
            minimized_from: wire::display::WindowState::Normal,
            normal_rect: rect,
            pending_configure: None,
            retired_grant_id: None,
            pending_close: None,
            serial: 0,
            behind_tab: false,
        }
    }

    /// UTF-8 title shown on the Cell Tab.
    pub fn title(&self) -> &str {
        &self.title
    }

    pub const fn icon(&self) -> ostd::display::linear::icons::LineIcon {
        self.icon
    }

    pub fn set_icon(&mut self, icon: ostd::display::linear::icons::LineIcon) {
        self.icon = icon;
    }

    /// Everything this surface may paint on screen: content plus compositor
    /// chrome (window frame and margin, or a panel's shadow margin).
    pub fn paint_region(&self) -> wire::display::Rect {
        let rect = self.screen_rect();
        if self.is_window_managed() {
            crate::window_decoration::bounds(rect)
        } else if self.role == wire::display::SurfaceRole::Panel {
            crate::window_decoration::panel_region(rect)
        } else {
            rect
        }
    }

    /// Replace the title after the caller has validated the fixed protocol frame.
    pub fn set_title(&mut self, title: &str) {
        self.title.clear();
        self.title.push_str(title);
    }

    /// Current compositor-managed state.
    pub const fn state(&self) -> wire::display::WindowState {
        self.state
    }

    /// True only while content may participate in scanout.
    pub const fn is_visible_for_paint(&self) -> bool {
        matches!(
            self.state,
            wire::display::WindowState::Normal | wire::display::WindowState::Maximized
        )
    }

    /// True while the surface is on screen: visible and not behind another tab.
    pub const fn is_shown(&self) -> bool {
        self.is_visible_for_paint() && !self.behind_tab
    }

    /// Hide or show the surface behind the shown tab of its frame. Only
    /// `crate::stacking` decides this, from the frame registry.
    pub fn set_behind_tab(&mut self, behind: bool) {
        self.behind_tab = behind;
    }

    /// Whether a configure or a replaced grant still blocks the next proposal.
    pub const fn is_configuring(&self) -> bool {
        self.pending_configure.is_some() || self.retired_grant_id.is_some()
    }

    /// True only for a shown interactive surface.
    pub fn accepts_input(&self) -> bool {
        (self.role == SurfaceRole::Interactive || self.role == SurfaceRole::Panel)
            && self.is_shown()
    }

    /// Background surfaces are deliberately outside the window-manager state machine.
    pub fn is_window_managed(&self) -> bool {
        self.role == SurfaceRole::Interactive
    }

    /// Move a normal surface and retain that position for a later restore.
    pub fn move_to(&mut self, x: i32, y: i32) {
        self.x = x;
        self.y = y;
        if self.state == wire::display::WindowState::Normal {
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
