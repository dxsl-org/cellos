use ostd::display::linear::icons::{paint_icon, LineIcon, ICON_SIZE};
use ostd::display::linear::{Bgra, Canvas, Radii, RoundRect, CONTROL_RADIUS};
use wire::display::Rect;

use super::palette::ChromePalette;
use super::title::TitleFont;
use super::{end, icon_box, shapes, tab_slot, Control, Tabs, RADIUS, TITLE};

/// Glyph box side. Odd, so the close diagonals cross on a single pixel
/// instead of a 2x2 blob.
const GLYPH: i32 = 9;
/// Space between the app icon box and the title, and after the title.
const TITLE_GAP: i32 = 6;

/// One Cell Tab: the window's app icon and title.
#[derive(Clone, Copy)]
pub struct TabLabel<'a> {
    pub icon: LineIcon,
    pub title: &'a str,
    /// The window is closing: its tab stays, faded, until it is gone.
    pub dimmed: bool,
}

/// What a frame's chrome shows this frame.
pub struct Style<'a> {
    pub active: bool,
    pub palette: &'static ChromePalette,
    /// The frame's tabs in order; at least one.
    pub tabs: &'a [TabLabel<'a>],
    /// Index into `tabs` of the shown window.
    pub selected: usize,
    /// A lone tab's slide along the top edge ([`Tabs::offset`]).
    pub offset: u32,
    /// `Some(open)` while the pointer is over the shown tab's app icon;
    /// `open` while its window menu is showing.
    pub menu_button: Option<bool>,
}

impl Style<'_> {
    fn strip(&self) -> Tabs {
        Tabs {
            count: self.tabs.len().max(1),
            selected: self.selected,
            offset: self.offset,
        }
    }
}

/// Paint shadow, tab fills, hairlines, app icons and titles inside `dirty`.
/// The shown tab takes the frame's focus colours; the other tabs of a stack
/// sit back in the palette's stacked-tab fill.
///
/// Content pixels are not touched except where the body outline covers them;
/// callers blit content afterwards with [`super::content_row_insets`].
pub(super) fn paint(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    content: Rect,
    dirty: Rect,
    style: &Style<'_>,
    font: &mut TitleFont,
) {
    let colors = if style.active {
        &style.palette.active
    } else {
        &style.palette.inactive
    };
    let tabs = style.strip();
    let [strip, body] = shapes(content, tabs);
    {
        let mut canvas = Canvas::new(pixels, width, height, width as usize * 4)
            .clip_to(dirty.x, dirty.y, dirty.w, dirty.h);
        canvas.drop_shadow(&[strip, body], &style.palette.shadow);
        canvas.stroke_round(&body, colors.outline);
        for (index, label) in style.tabs.iter().enumerate() {
            let shown = index == style.selected;
            let slot = slot_shape(content, tabs, index);
            let fill = if shown {
                colors.tab
            } else {
                style.palette.stacked
            };
            canvas.fill_round(
                &RoundRect {
                    h: slot.h.saturating_sub(1),
                    ..slot
                },
                fill,
            );
            canvas.stroke_round(&slot, colors.outline);
            let button = icon_box(tab_slot(content, tabs, index));
            let mut ink = if shown {
                colors.glyph
            } else {
                style.palette.inactive.glyph
            };
            if let (true, Some(open)) = (shown, style.menu_button) {
                let fill = if open {
                    style.palette.pressed
                } else {
                    style.palette.hover
                };
                let shape = RoundRect::new(
                    button.x,
                    button.y,
                    button.w,
                    button.h,
                    Radii::all(CONTROL_RADIUS),
                );
                canvas.fill_round(&shape, fill);
                ink = style.palette.active.glyph;
            }
            let inset = (button.w.saturating_sub(ICON_SIZE) / 2) as i32;
            paint_icon(
                &mut canvas,
                label.icon,
                button.x + inset,
                button.y + inset,
                ink,
            );
        }
    }
    for (index, label) in style.tabs.iter().enumerate() {
        let slot = tab_slot(content, tabs, index);
        let button = icon_box(slot);
        let left = end(button.x, button.w) + TITLE_GAP;
        let right = end(slot.x, slot.w) - TITLE_GAP;
        if right <= left {
            continue;
        }
        let area = Rect {
            x: left,
            y: slot.y + 1,
            w: (right - left) as u32,
            h: (TITLE - 2) as u32,
        };
        let ink = if label.dimmed {
            style.palette.inactive.glyph
        } else if index == style.selected {
            colors.title
        } else {
            style.palette.inactive.title
        };
        font.draw(pixels, width, height, area, dirty, label.title, ink);
    }
}

/// Outline of tab `index`: only the strip's outer top corners are rounded.
fn slot_shape(content: Rect, tabs: Tabs, index: usize) -> RoundRect {
    let slot = tab_slot(content, tabs, index);
    let first = index == 0;
    let last = index + 1 >= tabs.count;
    let radii = Radii {
        top_left: if first { RADIUS } else { 0 },
        top_right: if last { RADIUS } else { 0 },
        ..Radii::default()
    };
    RoundRect::new(slot.x, slot.y, slot.w, slot.h, radii)
}

/// 1px glyph for a window-menu item, centred in the square `cell`.
pub(super) fn paint_glyph(
    canvas: &mut Canvas<'_>,
    cell: Rect,
    control: Control,
    restore: bool,
    color: Bgra,
) {
    let x = cell.x + (cell.w as i32 - GLYPH) / 2;
    let y = cell.y + (cell.h as i32 - GLYPH) / 2;
    let last = GLYPH - 1;
    match control {
        Control::Close => {
            canvas.line(x, y, x + last, y + last, color);
            canvas.line(x + last, y, x, y + last, color);
        }
        Control::Maximize if restore => {
            // A front frame with the back frame's top and right edges peeking
            // out: the classic "restore down" glyph.
            canvas.hline(x + 2, y, 7, color);
            canvas.vline(x + 8, y, 7, color);
            canvas.stroke_rect(x, y + 2, 7, 7, color);
        }
        Control::Maximize => {
            let frame = RoundRect::new(x, y, GLYPH as u32, GLYPH as u32, Radii::all(2));
            canvas.stroke_round(&frame, color);
        }
        Control::Minimize => canvas.hline(x, y + GLYPH / 2, GLYPH as u32, color),
    }
}
