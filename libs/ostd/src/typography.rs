// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//! Static Inter 4.1 GUI faces and JetBrains Mono 2.304 terminal faces.
//!
//! Faces are parsed lazily, once per context. Keep a context across frames so
//! cached glyphs survive repaint. Font licenses accompany the original assets.

use alloc::collections::BTreeMap;
use unicode_normalization::UnicodeNormalization;

use crate::font_atlas::GlyphAtlas;

pub use unicode_normalization::char::is_combining_mark;

pub const INTER_REGULAR: &[u8] = include_bytes!("../assets/fonts/inter/Inter-Regular.ttf");
pub const INTER_SEMIBOLD: &[u8] = include_bytes!("../assets/fonts/inter/Inter-SemiBold.ttf");
pub const INTER_ITALIC: &[u8] = include_bytes!("../assets/fonts/inter/Inter-Italic.ttf");
pub const INTER_SEMIBOLD_ITALIC: &[u8] = include_bytes!("../assets/fonts/inter/Inter-SemiBoldItalic.ttf");
pub const JETBRAINS_MONO_REGULAR: &[u8] = include_bytes!("../assets/fonts/jetbrains-mono/JetBrainsMono-Regular.ttf");
pub const JETBRAINS_MONO_BOLD: &[u8] = include_bytes!("../assets/fonts/jetbrains-mono/JetBrainsMono-Bold.ttf");

/// Canonically compose text without allocating a copy of the complete string.
pub fn normalized_chars(text: &str) -> impl Iterator<Item = char> + '_ {
    text.nfc()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FontFace {
    UiRegular,
    UiSemibold,
    UiItalic,
    UiSemiboldItalic,
    MonoRegular,
    MonoBold,
}

impl FontFace {
    pub fn bytes(self) -> &'static [u8] {
        match self {
            Self::UiRegular => INTER_REGULAR,
            Self::UiSemibold => INTER_SEMIBOLD,
            Self::UiItalic => INTER_ITALIC,
            Self::UiSemiboldItalic => INTER_SEMIBOLD_ITALIC,
            Self::MonoRegular => JETBRAINS_MONO_REGULAR,
            Self::MonoBold => JETBRAINS_MONO_BOLD,
        }
    }
}

/// Application-owned font state; unused faces do not consume heap space.
#[derive(Default)]
pub struct TextFonts {
    faces: BTreeMap<FontFace, GlyphAtlas>,
}

impl TextFonts {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn atlas(&mut self, face: FontFace) -> &mut GlyphAtlas {
        self.faces.entry(face).or_insert_with(|| {
            GlyphAtlas::from_static(face.bytes()).expect("invalid bundled font")
        })
    }

    pub fn measure(&mut self, face: FontFace, text: &str, px: f32) -> f32 {
        self.atlas(face).measure(text, px)
    }

    pub fn advance(&mut self, face: FontFace, c: char, px: f32) -> f32 {
        self.atlas(face).advance(c, px)
    }

    pub fn kerning(&mut self, face: FontFace, left: char, right: char, px: f32) -> f32 {
        self.atlas(face).kerning(left, right, px)
    }

    pub fn line_height(&mut self, face: FontFace, px: f32) -> f32 {
        self.atlas(face).line_height(px)
    }

    pub fn ascender(&mut self, face: FontFace, px: f32) -> f32 {
        self.atlas(face).ascender(px)
    }

    /// Paint a line into a BGRA buffer, clipped to the surface and viewport.
    /// `y` is the line top; returns the same advance as `measure`.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_text(
        &mut self,
        pixels: &mut [u8],
        width: u32,
        height: u32,
        stride: usize,
        x: f32,
        y: f32,
        text: &str,
        face: FontFace,
        px: f32,
        color: [u8; 4],
        clip: (i32, i32, i32, i32),
    ) -> f32 {
        self.atlas(face).draw_text(pixels, width, height, stride, x, y, text, px, color, clip)
    }
}
