//! The system theme: one palette for every ViUI app in the session.
//!
//! The compositor announces the palette chosen in the shell; ostd records it
//! from whatever path receives compositor frames. Runners, `PaintCx` and the
//! surface components default to [`system`] and repaint when [`serial`]
//! changes. An app that wants its own look passes its theme explicitly
//! (`ViApp::set_theme`) or draws itself.

use super::{SystemPalette, ViTheme, DARK_THEME, LIGHT_THEME, PAPER_THEME};
use api::display::AppearancePalette;

impl SystemPalette {
    pub const fn from_appearance(palette: AppearancePalette) -> Self {
        match palette {
            AppearancePalette::Light => Self::Light,
            AppearancePalette::Dark => Self::Dark,
            AppearancePalette::Paper => Self::Paper,
        }
    }

    /// The shared theme instance for this palette.
    pub fn theme(self) -> &'static dyn ViTheme {
        match self {
            Self::Light => &LIGHT_THEME,
            Self::Dark => &DARK_THEME,
            Self::Paper => &PAPER_THEME,
        }
    }
}

/// Palette currently chosen for the session (Light before any announcement).
pub fn system_palette() -> SystemPalette {
    SystemPalette::from_appearance(ostd::display::appearance::palette())
}

/// Theme for the session palette.
pub fn system() -> &'static dyn ViTheme {
    system_palette().theme()
}

/// Changes whenever the session palette changes; compare with the value seen
/// at the last paint to know when to repaint.
pub fn serial() -> u32 {
    ostd::display::appearance::serial()
}
