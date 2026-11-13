//! CellOS Linear icon set: a 16x16 grid drawn only with 1px strokes on whole
//! pixels — no fills, no anti-aliasing — so every icon stays sharp at any
//! position and keeps the weight of the window chrome. Circles use even
//! diameters: an odd one degenerates into an octagon at this size.
//!
//! Lives in ostd so the compositor draws the same app icons on Cell Tabs that
//! ViUI apps draw in their own UI.

use wire::mime::MimeType;

use super::{Bgra, Canvas, Radii, RoundRect};

/// Side of the icon grid in pixels.
pub const ICON_SIZE: u32 = 16;

/// Wire values (`as u8`) are part of `SET_ICON`; append new icons, never reorder.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineIcon {
    Application,
    Appearance,
    Blocks,
    Code,
    Counter,
    Dashboard,
    Document,
    Folder,
    Globe,
    Image,
    Pulse,
    Search,
    Terminal,
}

impl LineIcon {
    /// Every icon, in wire order.
    pub const ALL: [Self; 13] = [
        Self::Application,
        Self::Appearance,
        Self::Blocks,
        Self::Code,
        Self::Counter,
        Self::Dashboard,
        Self::Document,
        Self::Folder,
        Self::Globe,
        Self::Image,
        Self::Pulse,
        Self::Search,
        Self::Terminal,
    ];

    /// The icon with wire value `value`, if any.
    pub fn from_u8(value: u8) -> Option<Self> {
        Self::ALL.get(usize::from(value)).copied()
    }

    pub const fn for_mime(mime: MimeType) -> Self {
        match mime {
            MimeType::TextHtml => Self::Globe,
            MimeType::TextSource => Self::Code,
            MimeType::ImagePng | MimeType::ImageJpeg | MimeType::ImagePpm => Self::Image,
            MimeType::ApplicationElf => Self::Application,
            MimeType::InodeDirectory => Self::Folder,
            MimeType::TextPlain
            | MimeType::TextMarkdown
            | MimeType::ApplicationPdf
            | MimeType::Unknown => Self::Document,
        }
    }
}

/// Draw `icon` with its grid's top-left at `(x, y)` on a linear canvas.
pub fn paint_icon(c: &mut Canvas<'_>, icon: LineIcon, x: i32, y: i32, ink: Bgra) {
    let rect = |c: &mut Canvas<'_>, rx: i32, ry: i32, w: u32, h: u32, r: u32| {
        c.stroke_round(&RoundRect::new(x + rx, y + ry, w, h, Radii::all(r)), ink);
    };
    let path = |c: &mut Canvas<'_>, points: &[(i32, i32)]| {
        for pair in points.windows(2) {
            let ((x0, y0), (x1, y1)) = (pair[0], pair[1]);
            c.line(x + x0, y + y0, x + x1, y + y1, ink);
        }
    };
    match icon {
        LineIcon::Application => {
            rect(c, 0, 1, 16, 14, 2);
            c.hline(x + 1, y + 4, 14, ink);
            c.hline(x + 3, y + 2, 1, ink);
            c.hline(x + 5, y + 2, 1, ink);
        }
        LineIcon::Appearance => {
            rect(c, 0, 0, 16, 16, 8);
            c.vline(x + 8, y + 1, 14, ink);
            for row in [4, 7, 10] {
                c.hline(x + 9, y + row, 5, ink);
            }
        }
        LineIcon::Blocks => {
            for (bx, by) in [(0, 9), (4, 9), (8, 9), (8, 5)] {
                rect(c, bx, by, 5, 5, 0);
            }
        }
        LineIcon::Code => {
            path(c, &[(5, 4), (1, 8), (5, 12)]);
            path(c, &[(10, 4), (14, 8), (10, 12)]);
            c.line(x + 9, y + 2, x + 6, y + 14, ink);
        }
        LineIcon::Counter => {
            rect(c, 0, 0, 15, 15, 3);
            c.hline(x + 4, y + 7, 7, ink);
            c.vline(x + 7, y + 4, 7, ink);
        }
        LineIcon::Dashboard => {
            rect(c, 0, 1, 16, 14, 2);
            for (bx, top) in [(4, 9), (7, 5), (10, 7)] {
                c.vline(x + bx, y + top, (12 - top) as u32, ink);
            }
        }
        LineIcon::Document => {
            path(c, &[(9, 0), (2, 0), (2, 15), (13, 15), (13, 4), (9, 0)]);
            path(c, &[(9, 0), (9, 4), (13, 4)]);
            c.hline(x + 5, y + 8, 6, ink);
            c.hline(x + 5, y + 11, 6, ink);
        }
        LineIcon::Folder => {
            path(c, &[(0, 4), (0, 2), (5, 2), (7, 4)]);
            rect(c, 0, 4, 16, 11, 1);
        }
        LineIcon::Globe => {
            rect(c, 0, 0, 16, 16, 8);
            rect(c, 4, 0, 8, 16, 4);
            c.hline(x + 1, y + 8, 14, ink);
        }
        LineIcon::Image => {
            rect(c, 0, 1, 16, 14, 2);
            path(c, &[(2, 12), (6, 7), (9, 10), (11, 8), (14, 11)]);
            rect(c, 10, 4, 3, 3, 0);
        }
        LineIcon::Pulse => path(c, &[(0, 8), (4, 8), (6, 3), (9, 13), (11, 8), (15, 8)]),
        LineIcon::Search => {
            rect(c, 1, 1, 10, 10, 5);
            c.line(x + 10, y + 10, x + 14, y + 14, ink);
        }
        LineIcon::Terminal => {
            rect(c, 0, 1, 16, 14, 2);
            path(c, &[(3, 5), (5, 7), (3, 9)]);
            c.hline(x + 7, y + 9, 5, ink);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_values_round_trip_and_unknown_values_are_refused() {
        for (value, icon) in LineIcon::ALL.iter().enumerate() {
            assert_eq!(*icon as u8 as usize, value);
            assert_eq!(LineIcon::from_u8(value as u8), Some(*icon));
        }
        assert_eq!(LineIcon::from_u8(LineIcon::ALL.len() as u8), None);
    }
}
