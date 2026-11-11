//! Chrome colours per system palette (CellOS Linear, contracts.md "Appearance").
//!
//! The tab and frame copy Haiku's geometry, not its colours: every value here
//! derives from the CellOS Light / Dark / Paper tokens. Hairlines sit a step
//! darker (lighter on Dark) than the `border` token so a window separates from
//! any wallpaper; tab text keeps >= 4.5:1 and control glyphs >= 3:1 (WCAG
//! non-text contrast) against their background in every state.

use api::display::AppearancePalette;
use ostd::display::linear::{Bgra, Shadow};

/// Colours for one focus state.
pub(crate) struct FocusColors {
    pub(crate) tab: Bgra,
    pub(crate) outline: Bgra,
    pub(crate) glyph: Bgra,
    pub(crate) title: Bgra,
}

pub(crate) struct ChromePalette {
    pub(crate) active: FocusColors,
    pub(crate) inactive: FocusColors,
    /// App-icon and menu-item background under the pointer, and while open.
    pub(crate) hover: Bgra,
    pub(crate) pressed: Bgra,
    /// Fill of a stacked tab that is not the shown one: a step behind both
    /// focus states, drawn with the inactive title and glyph colours.
    pub(crate) stacked: Bgra,
    /// Outline of a Win+drag's ghost frame and of the tabs it would join.
    pub(crate) accent: Bgra,
    pub(crate) shadow: Shadow,
}

const fn bgra(r: u8, g: u8, b: u8) -> Bgra {
    [b, g, r, 0xff]
}

/// Soft shadow from a light above the screen: fades over 10px around the
/// caster moved 3px down (13px below, inside `SHADOW_MARGIN`). Black, so it
/// darkens whatever lies below; only its strength varies by palette.
const fn shadow(opacity: u8) -> Shadow {
    Shadow {
        color: bgra(0, 0, 0),
        opacity,
        spread: 10,
        offset_x: 0,
        offset_y: 3,
    }
}

pub(crate) const LIGHT: ChromePalette = ChromePalette {
    active: FocusColors {
        tab: bgra(0xff, 0xff, 0xff),
        outline: bgra(0x9a, 0xa0, 0xad),
        glyph: bgra(0x3c, 0x41, 0x50),
        title: bgra(0x14, 0x14, 0x1c),
    },
    inactive: FocusColors {
        tab: bgra(0xeb, 0xed, 0xf2),
        outline: bgra(0xc3, 0xc7, 0xd0),
        glyph: bgra(0x6e, 0x73, 0x83),
        title: bgra(0x5f, 0x64, 0x73),
    },
    hover: bgra(0xe2, 0xe5, 0xec),
    pressed: bgra(0xd0, 0xd4, 0xdd),
    stacked: bgra(0xdf, 0xe2, 0xe9),
    accent: bgra(0x25, 0x63, 0xeb),
    shadow: shadow(64),
};

pub(crate) const DARK: ChromePalette = ChromePalette {
    active: FocusColors {
        tab: bgra(0x26, 0x26, 0x34),
        outline: bgra(0x55, 0x58, 0x70),
        glyph: bgra(0xc8, 0xca, 0xd6),
        title: bgra(0xf5, 0xf5, 0xfa),
    },
    inactive: FocusColors {
        tab: bgra(0x16, 0x16, 0x1e),
        outline: bgra(0x37, 0x37, 0x4b),
        glyph: bgra(0x7a, 0x7f, 0x8e),
        title: bgra(0x9b, 0xa0, 0xaf),
    },
    hover: bgra(0x36, 0x36, 0x4a),
    pressed: bgra(0x44, 0x44, 0x5c),
    stacked: bgra(0x0e, 0x0e, 0x14),
    accent: bgra(0x6e, 0x9b, 0xff),
    shadow: shadow(120),
};

pub(crate) const PAPER: ChromePalette = ChromePalette {
    active: FocusColors {
        tab: bgra(0xfa, 0xf8, 0xf3),
        outline: bgra(0xad, 0xa6, 0x96),
        glyph: bgra(0x4a, 0x48, 0x43),
        title: bgra(0x2b, 0x2b, 0x2a),
    },
    inactive: FocusColors {
        tab: bgra(0xe8, 0xe4, 0xda),
        outline: bgra(0xd1, 0xcb, 0xbd),
        glyph: bgra(0x74, 0x70, 0x6a),
        title: bgra(0x60, 0x5e, 0x5a),
    },
    hover: bgra(0xe6, 0xe1, 0xd5),
    pressed: bgra(0xd6, 0xd0, 0xc1),
    stacked: bgra(0xdd, 0xd8, 0xcc),
    accent: bgra(0x2f, 0x5d, 0xa8),
    shadow: shadow(64),
};

pub(crate) const fn chrome(palette: AppearancePalette) -> &'static ChromePalette {
    match palette {
        AppearancePalette::Light => &LIGHT,
        AppearancePalette::Dark => &DARK,
        AppearancePalette::Paper => &PAPER,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn luminance(c: Bgra) -> f64 {
        let lin = |v: u8| {
            let s = f64::from(v) / 255.0;
            if s <= 0.04045 {
                s / 12.92
            } else {
                ((s + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * lin(c[2]) + 0.7152 * lin(c[1]) + 0.0722 * lin(c[0])
    }

    fn contrast(a: Bgra, b: Bgra) -> f64 {
        let (la, lb) = (luminance(a), luminance(b));
        (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
    }

    #[test]
    fn tab_titles_meet_text_contrast_in_every_palette_and_focus_state() {
        for palette in [
            AppearancePalette::Light,
            AppearancePalette::Dark,
            AppearancePalette::Paper,
        ] {
            let chrome = chrome(palette);
            for colors in [&chrome.active, &chrome.inactive] {
                let ratio = contrast(colors.title, colors.tab);
                assert!(ratio >= 4.5, "{palette:?}: title contrast {ratio:.2}");
                assert!(
                    contrast(colors.glyph, colors.tab) >= 3.0,
                    "{palette:?}: glyph contrast"
                );
            }
            // Tabs behind the shown one use the inactive inks on their fill.
            let ratio = contrast(chrome.inactive.title, chrome.stacked);
            assert!(
                ratio >= 4.5,
                "{palette:?}: stacked title contrast {ratio:.2}"
            );
            assert!(
                contrast(chrome.inactive.glyph, chrome.stacked) >= 3.0,
                "{palette:?}: stacked glyph contrast"
            );
            // The drag outline stands out from the tabs it frames.
            assert!(
                contrast(chrome.accent, chrome.active.tab) >= 3.0,
                "{palette:?}: accent"
            );
            // A hovered control draws the active glyph on the hover/pressed fill.
            for fill in [chrome.hover, chrome.pressed] {
                assert!(
                    contrast(chrome.active.glyph, fill) >= 3.0,
                    "{palette:?}: hovered glyph"
                );
            }
        }
    }
}
