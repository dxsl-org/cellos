//! `ViTheme` trait and built-in theme implementations.
//!
//! Widgets read colors/spacing from `cx.theme` instead of hardcoding values.

use crate::canvas::Color;

// ─── ViTheme trait ───────────────────────────────────────────────────────────

pub trait ViTheme: 'static {
    // Surface colours
    fn bg(&self) -> Color;
    fn surface(&self) -> Color; // card / panel background
    fn border(&self) -> Color;

    // Text colours
    fn text_primary(&self) -> Color;
    fn text_secondary(&self) -> Color;

    // Interactive colours
    fn accent(&self) -> Color;
    fn button_normal(&self) -> Color;
    fn button_hovered(&self) -> Color;
    fn button_pressed(&self) -> Color;
    fn input_bg(&self) -> Color;
    fn input_focused_bg(&self) -> Color;
    fn input_focused_border(&self) -> Color;

    // Spacing
    fn padding_sm(&self) -> f32 {
        4.0
    }
    fn padding_md(&self) -> f32 {
        8.0
    }
    fn padding_lg(&self) -> f32 {
        16.0
    }

    // Font sizes (px). 0 inherits the GUI FontContext size (Inter 16px by default).
    fn font_size_body(&self) -> u16 {
        0
    }
    fn font_size_heading(&self) -> u16 {
        0
    }

    // ─── Widget-specific tokens (default impls derive from base tokens) ────

    /// Fill color for the ProgressBar filled region.
    fn progress_fill(&self) -> Color {
        self.accent()
    }

    /// Track (rail) background color for Slider.
    fn slider_track(&self) -> Color {
        self.border()
    }

    /// Background highlight for a selected ListView row.
    ///
    /// Uses a semi-transparent version of `accent()` so the row text remains
    /// readable regardless of background color.
    fn list_selected_bg(&self) -> Color {
        self.accent().with_alpha(60)
    }

    /// Foreground (text) color for a selected ListView row.
    fn list_selected_fg(&self) -> Color {
        self.text_primary()
    }

    /// Check-mark color inside a CheckBox.
    fn check_color(&self) -> Color {
        self.accent()
    }

    /// Horizontal/vertical divider line color.
    fn divider(&self) -> Color {
        self.border()
    }

    // ─── Window & Tab Chrome tokens ──────────────────────────────────────────

    /// Background color for the active session tab.
    fn tab_active_bg(&self) -> Color {
        self.surface()
    }

    /// Text/icon color for the active session tab.
    fn tab_active_fg(&self) -> Color {
        self.text_primary()
    }

    /// Background color for an inactive session tab.
    fn tab_inactive_bg(&self) -> Color {
        self.bg()
    }

    /// Text/icon color for an inactive session tab.
    fn tab_inactive_fg(&self) -> Color {
        self.text_secondary()
    }

    /// Border color between tabs and frame header.
    fn tab_border(&self) -> Color {
        self.border()
    }

    /// Hover highlight color for tab close button.
    fn tab_close_hover(&self) -> Color {
        self.button_hovered()
    }

    // ─── Taskbar & Spotlight tokens ─────────────────────────────────────────

    /// Background color for the bottom taskbar.
    fn taskbar_bg(&self) -> Color {
        self.surface()
    }

    /// Foreground (text/icon) color for the bottom taskbar.
    fn taskbar_fg(&self) -> Color {
        self.text_primary()
    }

    /// Border color along the top edge of the taskbar.
    fn taskbar_border(&self) -> Color {
        self.border()
    }

    /// Background color for the Spotlight search HUD window.
    fn spotlight_bg(&self) -> Color {
        self.surface()
    }

    /// Border color around the Spotlight HUD window.
    fn spotlight_border(&self) -> Color {
        self.accent()
    }

    // ─── Focus & Status tokens ──────────────────────────────────────────────

    /// Focus ring outline color for keyboard navigation.
    fn focus_ring(&self) -> Color {
        self.accent()
    }

    /// Unsaved/dirty document badge color.
    fn badge_dirty(&self) -> Color {
        Color::rgb(217, 119, 6)
    }

    /// Attention/notification badge color.
    fn badge_attention(&self) -> Color {
        Color::rgb(37, 99, 235)
    }

    /// Error badge or refusal highlight color.
    fn badge_error(&self) -> Color {
        Color::rgb(220, 38, 38)
    }

    // ─── Reader tokens (Ocel / Document viewers) ─────────────────────────────

    /// Background color for document reader mode.
    fn reader_bg(&self) -> Color {
        self.bg()
    }

    /// Primary reading text color.
    fn reader_text(&self) -> Color {
        self.text_primary()
    }

    /// Secondary reading subtext/caption color.
    fn reader_subtext(&self) -> Color {
        self.text_secondary()
    }

    /// Hyperlink color for document reader mode.
    fn reader_link(&self) -> Color {
        self.accent()
    }
}

// ─── DarkTheme ───────────────────────────────────────────────────────────────

pub struct DarkTheme;

impl ViTheme for DarkTheme {
    fn bg(&self) -> Color {
        Color::rgb(18, 18, 24)
    }
    fn surface(&self) -> Color {
        Color::rgb(28, 28, 38)
    }
    fn border(&self) -> Color {
        Color::rgb(55, 55, 75)
    }

    fn text_primary(&self) -> Color {
        Color::rgb(245, 245, 250)
    }
    fn text_secondary(&self) -> Color {
        Color::rgb(155, 160, 175)
    }

    fn accent(&self) -> Color {
        Color::rgb(80, 130, 240)
    }
    fn button_normal(&self) -> Color {
        Color::rgb(42, 42, 56)
    }
    fn button_hovered(&self) -> Color {
        Color::rgb(58, 58, 76)
    }
    fn button_pressed(&self) -> Color {
        Color::rgb(72, 72, 95)
    }
    fn input_bg(&self) -> Color {
        Color::rgb(22, 22, 30)
    }
    fn input_focused_bg(&self) -> Color {
        Color::rgb(18, 22, 45)
    }
    fn input_focused_border(&self) -> Color {
        Color::rgb(80, 130, 240)
    }

    fn tab_active_bg(&self) -> Color {
        Color::rgb(38, 38, 52)
    }
    fn tab_active_fg(&self) -> Color {
        Color::rgb(245, 245, 250)
    }
    fn tab_inactive_bg(&self) -> Color {
        Color::rgb(22, 22, 30)
    }
    fn tab_inactive_fg(&self) -> Color {
        Color::rgb(150, 155, 170)
    }
    fn taskbar_bg(&self) -> Color {
        Color::rgb(22, 22, 30)
    }
    fn taskbar_fg(&self) -> Color {
        Color::rgb(240, 240, 245)
    }
    fn spotlight_bg(&self) -> Color {
        Color::rgb(28, 28, 38)
    }
    fn spotlight_border(&self) -> Color {
        Color::rgb(80, 130, 240)
    }
    fn focus_ring(&self) -> Color {
        Color::rgb(80, 130, 240)
    }
    fn reader_bg(&self) -> Color {
        Color::rgb(22, 22, 30)
    }
    fn reader_text(&self) -> Color {
        Color::rgb(240, 240, 245)
    }
    fn reader_subtext(&self) -> Color {
        Color::rgb(155, 160, 175)
    }

    fn reader_link(&self) -> Color {
        Color::rgb(90, 145, 255)
    }
}

// ─── LightTheme ──────────────────────────────────────────────────────────────

pub struct LightTheme;

impl ViTheme for LightTheme {
    fn bg(&self) -> Color {
        Color::rgb(245, 245, 248)
    }
    fn surface(&self) -> Color {
        Color::WHITE
    }
    fn border(&self) -> Color {
        Color::rgb(215, 218, 224)
    }

    fn text_primary(&self) -> Color {
        Color::rgb(20, 20, 28)
    }
    fn text_secondary(&self) -> Color {
        Color::rgb(95, 100, 115)
    }

    fn accent(&self) -> Color {
        Color::rgb(35, 90, 215)
    }
    fn button_normal(&self) -> Color {
        Color::rgb(230, 232, 238)
    }
    fn button_hovered(&self) -> Color {
        Color::rgb(218, 222, 230)
    }
    fn button_pressed(&self) -> Color {
        Color::rgb(200, 205, 218)
    }
    fn input_bg(&self) -> Color {
        Color::WHITE
    }
    fn input_focused_bg(&self) -> Color {
        Color::rgb(242, 246, 255)
    }
    fn input_focused_border(&self) -> Color {
        Color::rgb(35, 90, 215)
    }

    fn tab_active_bg(&self) -> Color {
        Color::WHITE
    }
    fn tab_active_fg(&self) -> Color {
        Color::rgb(20, 20, 28)
    }
    fn tab_inactive_bg(&self) -> Color {
        Color::rgb(235, 237, 242)
    }
    fn tab_inactive_fg(&self) -> Color {
        Color::rgb(90, 95, 110)
    }
    fn taskbar_bg(&self) -> Color {
        Color::rgb(240, 242, 246)
    }
    fn taskbar_fg(&self) -> Color {
        Color::rgb(20, 20, 28)
    }
    fn spotlight_bg(&self) -> Color {
        Color::WHITE
    }
    fn spotlight_border(&self) -> Color {
        Color::rgb(35, 90, 215)
    }
    fn focus_ring(&self) -> Color {
        Color::rgb(35, 90, 215)
    }
    fn reader_bg(&self) -> Color {
        Color::WHITE
    }
    fn reader_text(&self) -> Color {
        Color::rgb(20, 20, 28)
    }
    fn reader_subtext(&self) -> Color {
        Color::rgb(95, 100, 115)
    }

    fn reader_link(&self) -> Color {
        Color::rgb(30, 80, 200)
    }
}

// ─── KioskTheme ──────────────────────────────────────────────────────────────
//
// High contrast, large touch targets (≥44×44px), minimum font size 16px.

pub struct KioskTheme;

impl ViTheme for KioskTheme {
    fn bg(&self) -> Color {
        Color::BLACK
    }
    fn surface(&self) -> Color {
        Color::rgb(20, 20, 20)
    }
    fn border(&self) -> Color {
        Color::WHITE
    }

    fn text_primary(&self) -> Color {
        Color::WHITE
    }
    fn text_secondary(&self) -> Color {
        Color::GRAY
    }

    fn accent(&self) -> Color {
        Color::YELLOW
    }
    fn button_normal(&self) -> Color {
        Color::rgb(40, 40, 40)
    }
    fn button_hovered(&self) -> Color {
        Color::GRAY
    }
    fn button_pressed(&self) -> Color {
        Color::rgb(180, 160, 0)
    }
    fn input_bg(&self) -> Color {
        Color::rgb(10, 10, 10)
    }
    fn input_focused_bg(&self) -> Color {
        Color::rgb(0, 0, 30)
    }
    fn input_focused_border(&self) -> Color {
        Color::YELLOW
    }

    fn padding_sm(&self) -> f32 {
        8.0
    }
    fn padding_md(&self) -> f32 {
        16.0
    }
    fn padding_lg(&self) -> f32 {
        24.0
    }

    // Kiosk mode requires minimum 16 px body text for readability at distance.
    fn font_size_body(&self) -> u16 {
        18
    }
    fn font_size_heading(&self) -> u16 {
        22
    }

    // High-contrast green fill for progress (more visible than accent yellow).
    fn progress_fill(&self) -> Color {
        Color::rgb(0, 200, 100)
    }

    // Solid blue selection highlight (opaque for maximum contrast on kiosk displays).
    fn list_selected_bg(&self) -> Color {
        Color::rgb(0, 100, 200)
    }
}

// ─── RobotTheme ──────────────────────────────────────────────────────────────
//
// Industrial dark theme for embedded robot dashboards.
// Designed for low-ambient-light environments with blue accent.

pub struct RobotTheme;

impl ViTheme for RobotTheme {
    fn bg(&self) -> Color {
        Color::rgb(10, 12, 15)
    }
    fn surface(&self) -> Color {
        Color::rgb(22, 26, 30)
    }
    fn border(&self) -> Color {
        Color::rgb(80, 90, 100)
    }

    fn text_primary(&self) -> Color {
        Color::rgb(220, 225, 230)
    }
    fn text_secondary(&self) -> Color {
        Color::rgb(140, 150, 160)
    }

    fn accent(&self) -> Color {
        Color::rgb(0, 160, 255)
    }
    fn button_normal(&self) -> Color {
        Color::rgb(35, 40, 48)
    }
    fn button_hovered(&self) -> Color {
        Color::rgb(50, 60, 72)
    }
    fn button_pressed(&self) -> Color {
        Color::rgb(0, 120, 200)
    }
    fn input_bg(&self) -> Color {
        Color::rgb(15, 18, 22)
    }
    fn input_focused_bg(&self) -> Color {
        Color::rgb(10, 15, 40)
    }
    fn input_focused_border(&self) -> Color {
        Color::rgb(0, 160, 255)
    }

    fn padding_sm(&self) -> f32 {
        6.0
    }
    fn padding_md(&self) -> f32 {
        12.0
    }
    fn padding_lg(&self) -> f32 {
        20.0
    }

    fn font_size_body(&self) -> u16 {
        16
    }
    fn font_size_heading(&self) -> u16 {
        20
    }
}

// ─── Default theme instance ──────────────────────────────────────────────────

/// Default dark theme used when no theme is provided to PaintCx.
pub static DARK_THEME: DarkTheme = DarkTheme;

/// Default light theme instance.
pub static LIGHT_THEME: LightTheme = LightTheme;

// ─── PaperTheme ─────────────────────────────────────────────────────────────

/// Warm sepia palette inspired by book-print ink and cream paper (#F6F3EB).
/// Reduces eye strain for long reading and writing sessions.
#[derive(Debug, Clone, Copy, Default)]
pub struct PaperTheme;

impl ViTheme for PaperTheme {
    fn bg(&self) -> Color {
        Color::rgb(246, 243, 235) // #F6F3EB
    }
    fn surface(&self) -> Color {
        Color::rgb(239, 236, 227) // #EFECE3
    }
    fn border(&self) -> Color {
        Color::rgb(218, 213, 200) // #DAD5C8
    }

    fn text_primary(&self) -> Color {
        Color::rgb(43, 43, 42) // #2B2B2A - high contrast
    }
    fn text_secondary(&self) -> Color {
        Color::rgb(96, 94, 90) // #605E5A
    }

    fn accent(&self) -> Color {
        Color::rgb(156, 91, 40) // #9C5B28 (warm terracotta)
    }
    fn button_normal(&self) -> Color {
        Color::rgb(232, 228, 218)
    }
    fn button_hovered(&self) -> Color {
        Color::rgb(223, 218, 207)
    }
    fn button_pressed(&self) -> Color {
        Color::rgb(212, 206, 193)
    }
    fn input_bg(&self) -> Color {
        Color::rgb(250, 248, 243)
    }
    fn input_focused_bg(&self) -> Color {
        Color::rgb(255, 254, 250)
    }
    fn input_focused_border(&self) -> Color {
        Color::rgb(156, 91, 40)
    }

    fn tab_active_bg(&self) -> Color {
        Color::rgb(250, 248, 243)
    }
    fn tab_active_fg(&self) -> Color {
        Color::rgb(43, 43, 42)
    }
    fn tab_inactive_bg(&self) -> Color {
        Color::rgb(232, 228, 218)
    }
    fn tab_inactive_fg(&self) -> Color {
        Color::rgb(96, 94, 90)
    }

    fn taskbar_bg(&self) -> Color {
        Color::rgb(236, 232, 223)
    }
    fn taskbar_fg(&self) -> Color {
        Color::rgb(43, 43, 42)
    }

    fn spotlight_bg(&self) -> Color {
        Color::rgb(250, 248, 243)
    }
    fn spotlight_border(&self) -> Color {
        Color::rgb(156, 91, 40)
    }

    fn focus_ring(&self) -> Color {
        Color::rgb(156, 91, 40)
    }

    fn badge_dirty(&self) -> Color {
        Color::rgb(180, 83, 9)
    }
    fn badge_attention(&self) -> Color {
        Color::rgb(29, 78, 216)
    }
    fn badge_error(&self) -> Color {
        Color::rgb(185, 28, 28)
    }

    fn reader_bg(&self) -> Color {
        Color::rgb(246, 243, 235)
    }
    fn reader_text(&self) -> Color {
        Color::rgb(43, 43, 42)
    }
    fn reader_subtext(&self) -> Color {
        Color::rgb(96, 94, 90)
    }

    fn reader_link(&self) -> Color {
        Color::rgb(140, 75, 25)
    }
}

/// Default paper theme instance.
pub static PAPER_THEME: PaperTheme = PaperTheme;

// ─── SystemPalette ──────────────────────────────────────────────────────────

/// Standard system appearance palettes.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SystemPalette {
    #[default]
    Light = 0,
    Dark = 1,
    Paper = 2,
}

impl SystemPalette {
    pub const fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Dark,
            2 => Self::Paper,
            _ => Self::Light,
        }
    }

    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Light => "Light",
            Self::Dark => "Dark",
            Self::Paper => "Paper",
        }
    }
}

/// Create a boxed ViTheme instance for the given system palette mode.
pub fn theme_for_palette(palette: SystemPalette) -> alloc::boxed::Box<dyn ViTheme> {
    match palette {
        SystemPalette::Light => alloc::boxed::Box::new(LightTheme),
        SystemPalette::Dark => alloc::boxed::Box::new(DarkTheme),
        SystemPalette::Paper => alloc::boxed::Box::new(PaperTheme),
    }
}

/// Production system theme preference and revision state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThemePreferenceState {
    pub palette: SystemPalette,
    pub revision: u32,
}

impl Default for ThemePreferenceState {
    fn default() -> Self {
        Self {
            palette: SystemPalette::Light, // Default system appearance is Light
            revision: 1,
        }
    }
}

impl ThemePreferenceState {
    pub const fn new(palette: SystemPalette, revision: u32) -> Self {
        Self { palette, revision }
    }

    /// Apply an incoming theme update if its revision is strictly greater than current.
    /// Returns true if applied, false if rejected as stale or duplicate.
    pub fn apply_update(&mut self, new_palette: SystemPalette, new_revision: u32) -> bool {
        if new_revision > self.revision {
            self.palette = new_palette;
            self.revision = new_revision;
            true
        } else {
            false
        }
    }

    /// Update from a raw u8 mode (e.g. from IPC or config storage).
    /// Malformed or unknown mode falls back to documented default Light.
    pub fn update_from_raw(&mut self, raw_mode: u8, new_revision: u32) -> bool {
        let palette = SystemPalette::from_u8(raw_mode);
        self.apply_update(palette, new_revision)
    }

    /// Get a boxed ViTheme instance for the active palette.
    pub fn create_theme(&self) -> alloc::boxed::Box<dyn ViTheme> {
        theme_for_palette(self.palette)
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;

    /// Calculate WCAG 2.1 relative luminance for an sRGB Color.
    fn luminance(c: Color) -> f64 {
        let channel = |v: u8| -> f64 {
            let s = v as f64 / 255.0;
            if s <= 0.04045 {
                s / 12.92
            } else {
                ((s + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
    }

    /// Calculate WCAG 2.1 contrast ratio between two colors.
    fn contrast_ratio(c1: Color, c2: Color) -> f64 {
        let l1 = luminance(c1);
        let l2 = luminance(c2);
        let lighter = l1.max(l2);
        let darker = l1.min(l2);
        (lighter + 0.05) / (darker + 0.05)
    }

    #[test]
    fn test_wcag_contrast_ratios_across_all_palettes() {
        let themes: [(&str, alloc::boxed::Box<dyn ViTheme>); 3] = [
            ("Light", theme_for_palette(SystemPalette::Light)),
            ("Dark", theme_for_palette(SystemPalette::Dark)),
            ("Paper", theme_for_palette(SystemPalette::Paper)),
        ];

        for (name, theme) in &themes {
            // Text on Background
            let bg_contrast = contrast_ratio(theme.text_primary(), theme.bg());
            assert!(
                bg_contrast >= 4.5,
                "{name} theme text_primary on bg contrast {bg_contrast:.2} < 4.5"
            );

            // Text on Surface
            let surface_contrast = contrast_ratio(theme.text_primary(), theme.surface());
            assert!(
                surface_contrast >= 4.5,
                "{name} theme text_primary on surface contrast {surface_contrast:.2} < 4.5"
            );

            // Secondary text on Surface
            let sec_surface_contrast = contrast_ratio(theme.text_secondary(), theme.surface());
            assert!(
                sec_surface_contrast >= 4.5,
                "{name} theme text_secondary on surface contrast {sec_surface_contrast:.2} < 4.5"
            );

            // Reader text on Reader background
            let reader_contrast = contrast_ratio(theme.reader_text(), theme.reader_bg());
            assert!(
                reader_contrast >= 4.5,
                "{name} theme reader text contrast {reader_contrast:.2} < 4.5"
            );

            // Reader subtext on Reader background
            let reader_sub_contrast = contrast_ratio(theme.reader_subtext(), theme.reader_bg());
            assert!(
                reader_sub_contrast >= 4.5,
                "{name} theme reader subtext contrast {reader_sub_contrast:.2} < 4.5"
            );

            // Reader hyperlink on Reader background (strictly >= 5.0:1)
            let reader_link_contrast = contrast_ratio(theme.reader_link(), theme.reader_bg());
            assert!(
                reader_link_contrast >= 5.0,
                "{name} theme reader link contrast {reader_link_contrast:.2} < 5.0"
            );

            // Active Tab text on Active Tab background
            let tab_active_contrast = contrast_ratio(theme.tab_active_fg(), theme.tab_active_bg());
            assert!(
                tab_active_contrast >= 4.5,
                "{name} theme active tab contrast {tab_active_contrast:.2} < 4.5"
            );

            // Inactive Tab text on Inactive Tab background
            let tab_inactive_contrast = contrast_ratio(theme.tab_inactive_fg(), theme.tab_inactive_bg());
            assert!(
                tab_inactive_contrast >= 4.5,
                "{name} theme inactive tab contrast {tab_inactive_contrast:.2} < 4.5"
            );

            // Taskbar text on Taskbar background
            let taskbar_contrast = contrast_ratio(theme.taskbar_fg(), theme.taskbar_bg());
            assert!(
                taskbar_contrast >= 4.5,
                "{name} theme taskbar contrast {taskbar_contrast:.2} < 4.5"
            );
        }
    }

    #[test]
    fn test_palette_distinct_colors_and_roundtrip() {
        for val in 0..3u8 {
            let p = SystemPalette::from_u8(val);
            assert_eq!(p.as_u8(), val);
        }
        assert_eq!(SystemPalette::from_u8(99), SystemPalette::Light);

        let light = LightTheme;
        let dark = DarkTheme;
        let paper = PaperTheme;
        assert_ne!(light.bg(), dark.bg());
        assert_ne!(light.bg(), paper.bg());
        assert_ne!(dark.bg(), paper.bg());
    }

    #[test]
    fn test_theme_preference_precedence_and_revision() {
        let mut state = ThemePreferenceState::default();
        // Documented default is Light at revision 1
        assert_eq!(state.palette, SystemPalette::Light);
        assert_eq!(state.revision, 1);

        // Apply forward update to Paper
        assert!(state.apply_update(SystemPalette::Paper, 2));
        assert_eq!(state.palette, SystemPalette::Paper);
        assert_eq!(state.revision, 2);

        // Reject stale / duplicate revision
        assert!(!state.apply_update(SystemPalette::Dark, 2));
        assert_eq!(state.palette, SystemPalette::Paper);
        assert_eq!(state.revision, 2);

        // Reject older revision
        assert!(!state.apply_update(SystemPalette::Dark, 1));
        assert_eq!(state.palette, SystemPalette::Paper);
        assert_eq!(state.revision, 2);

        // Raw corrupt update falls back to Light
        assert!(state.update_from_raw(255, 3));
        assert_eq!(state.palette, SystemPalette::Light);
        assert_eq!(state.revision, 3);
    }
}
