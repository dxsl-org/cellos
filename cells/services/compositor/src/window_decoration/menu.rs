//! Window menu: the compositor-owned popup opened from a Cell Tab's app icon.
//!
//! Items come in sections, a hairline between them: the window's own
//! actions, closing (always the third row, where the hand expects it), then
//! grouping (stack onto a nearby window, detach this tab, untile). Which
//! items show depends on the frame; nothing unavailable shows.

use api::display::Rect;
use ostd::display::linear::icons::{paint_icon, LineIcon, ICON_SIZE};
use ostd::display::linear::{Bgra, Canvas, Radii, RoundRect, CONTROL_RADIUS, PANEL_RADIUS};

use super::palette::ChromePalette;
use super::raster::paint_glyph;
use super::title::TitleFont;
use super::{end, menu_button_rect, Control, Tabs, SHADOW_MARGIN};

/// Most items one menu shows.
pub const MAX_MENU_ITEMS: usize = 12;

/// What a window-menu item does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuAction {
    Maximize,
    Minimize,
    /// Stack this frame onto the frame showing this window.
    StackOnto(u64),
    /// Pull this tab out of its stack into a window of its own.
    Detach,
    /// Take this frame out of its tile group.
    Untile,
    /// Close this window (the shown tab of a stack).
    Close,
    /// Ask every tab of the stack to close, one after another.
    CloseAll,
    /// Ask every window of the tile group to close (each stack in turn).
    CloseGroup,
}

impl MenuAction {
    fn section(self) -> u8 {
        match self {
            Self::Maximize | Self::Minimize => 0,
            Self::Close | Self::CloseAll | Self::CloseGroup => 1,
            Self::StackOnto(_) | Self::Detach | Self::Untile => 2,
        }
    }
}

/// The items of one open menu, in display order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuItems {
    items: [Option<MenuAction>; MAX_MENU_ITEMS],
}

impl MenuItems {
    pub const fn new() -> Self {
        Self {
            items: [None; MAX_MENU_ITEMS],
        }
    }

    /// Append `action`; a full menu drops it.
    pub fn push(&mut self, action: MenuAction) {
        if let Some(slot) = self.items.iter_mut().find(|slot| slot.is_none()) {
            *slot = Some(action);
        }
    }

    pub fn get(&self, index: usize) -> Option<MenuAction> {
        self.items.get(index).copied().flatten()
    }

    pub fn len(&self) -> usize {
        self.items.iter().take_while(|slot| slot.is_some()).count()
    }

    pub fn iter(&self) -> impl Iterator<Item = MenuAction> + '_ {
        self.items.iter().map_while(|slot| *slot)
    }
}

/// How one item looks: its glyph or app icon, and its label.
#[derive(Clone, Copy)]
pub struct MenuEntry<'a> {
    pub glyph: MenuGlyph,
    /// Muted text before the label ("Onto" for a stack target).
    pub prefix: &'a str,
    pub label: &'a str,
}

#[derive(Clone, Copy)]
pub enum MenuGlyph {
    Control(Control, bool),
    Icon(LineIcon),
    Detach,
}

const WIDTH: u32 = 232;
const PAD: i32 = 4;
const ITEM_H: i32 = 28;
const SEPARATOR_H: i32 = 9;
const GLYPH_CELL: i32 = 28;
/// Gap between the icon box and the menu's top edge.
const DROP: i32 = 4;

/// Top of item `index` relative to the menu, a separator before each new section.
fn item_top(items: &MenuItems, index: usize) -> i32 {
    let separators = (1..=index)
        .filter(|&i| match (items.get(i - 1), items.get(i)) {
            (Some(before), Some(item)) => before.section() != item.section(),
            _ => false,
        })
        .count() as i32;
    PAD + index as i32 * ITEM_H + separators * SEPARATOR_H
}

fn height(items: &MenuItems) -> u32 {
    let count = items.len().max(1);
    (item_top(items, count - 1) + ITEM_H + PAD) as u32
}

/// The menu below the shown tab's app icon of the frame around `content`,
/// kept inside the `screen_w` x `screen_h` screen.
pub fn menu_rect(
    content: Rect,
    tabs: Tabs,
    items: &MenuItems,
    screen_w: u32,
    screen_h: u32,
) -> Rect {
    let button = menu_button_rect(content, tabs);
    let h = height(items);
    let max_x = (screen_w as i32 - WIDTH as i32).max(0);
    let max_y = (screen_h as i32 - h as i32).max(0);
    Rect {
        x: button.x.clamp(0, max_x),
        y: end(button.y, button.h).saturating_add(DROP).clamp(0, max_y),
        w: WIDTH,
        h,
    }
}

/// Everything the menu may paint, shadow included.
pub fn menu_region(menu: Rect) -> Rect {
    Rect {
        x: menu.x.saturating_sub(SHADOW_MARGIN),
        y: menu.y.saturating_sub(SHADOW_MARGIN),
        w: menu.w.saturating_add((SHADOW_MARGIN * 2) as u32),
        h: menu.h.saturating_add((SHADOW_MARGIN * 2) as u32),
    }
}

fn item_rect(menu: Rect, items: &MenuItems, index: usize) -> Rect {
    Rect {
        x: menu.x + PAD,
        y: menu.y + item_top(items, index),
        w: menu.w - (PAD * 2) as u32,
        h: ITEM_H as u32,
    }
}

/// Index into `items` of the item under `(x, y)`.
pub fn menu_item_at(menu: Rect, items: &MenuItems, x: i32, y: i32) -> Option<usize> {
    (0..items.len()).find(|&index| {
        let r = item_rect(menu, items, index);
        x >= r.x && x < end(r.x, r.w) && y >= r.y && y < end(r.y, r.h)
    })
}

/// Paint the menu `menu` inside `dirty`: shadow, rounded panel, one row per
/// entry (glyph or app icon, then label), section hairlines, the hovered row
/// highlighted.
#[allow(clippy::too_many_arguments)]
pub fn paint_menu(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    menu: Rect,
    dirty: Rect,
    items: &MenuItems,
    entries: &[MenuEntry<'_>],
    hover: Option<usize>,
    palette: &ChromePalette,
    font: &mut TitleFont,
) {
    let colors = &palette.active;
    {
        let panel = RoundRect::new(menu.x, menu.y, menu.w, menu.h, Radii::all(PANEL_RADIUS));
        let mut canvas = Canvas::new(pixels, width, height, width as usize * 4)
            .clip_to(dirty.x, dirty.y, dirty.w, dirty.h);
        canvas.drop_shadow(core::slice::from_ref(&panel), &palette.shadow);
        canvas.fill_round(&panel, colors.tab);
        canvas.stroke_round(&panel, colors.outline);
        for (index, entry) in entries.iter().enumerate() {
            let item = item_rect(menu, items, index);
            if index > 0 && item.y - item_rect(menu, items, index - 1).y > ITEM_H {
                let line_y = item.y - SEPARATOR_H / 2 - 1;
                canvas.hline(
                    menu.x + PAD + 4,
                    line_y,
                    menu.w - (PAD as u32 + 4) * 2,
                    colors.outline,
                );
            }
            if hover == Some(index) {
                let shape =
                    RoundRect::new(item.x, item.y, item.w, item.h, Radii::all(CONTROL_RADIUS));
                canvas.fill_round(&shape, palette.hover);
            }
            let cell = Rect {
                x: item.x,
                y: item.y,
                w: GLYPH_CELL as u32,
                h: item.h,
            };
            paint_entry_glyph(&mut canvas, cell, entry.glyph, colors.glyph);
        }
    }
    for (index, entry) in entries.iter().enumerate() {
        let item = item_rect(menu, items, index);
        let mut area = Rect {
            x: item.x + GLYPH_CELL + 2,
            y: item.y,
            w: item.w.saturating_sub(GLYPH_CELL as u32 + 6),
            h: item.h,
        };
        if !entry.prefix.is_empty() {
            font.draw(
                pixels,
                width,
                height,
                area,
                dirty,
                entry.prefix,
                palette.inactive.title,
            );
            let shift = (font.measure(entry.prefix) + 5).min(area.w);
            area = Rect {
                x: area.x + shift as i32,
                w: area.w - shift,
                ..area
            };
        }
        font.draw(
            pixels,
            width,
            height,
            area,
            dirty,
            entry.label,
            colors.title,
        );
    }
}

fn paint_entry_glyph(canvas: &mut Canvas<'_>, cell: Rect, glyph: MenuGlyph, color: Bgra) {
    match glyph {
        MenuGlyph::Control(control, restore) => paint_glyph(canvas, cell, control, restore, color),
        MenuGlyph::Icon(icon) => {
            let x = cell.x + (cell.w as i32 - ICON_SIZE as i32) / 2;
            let y = cell.y + (cell.h as i32 - ICON_SIZE as i32) / 2;
            paint_icon(canvas, icon, x, y, color);
        }
        MenuGlyph::Detach => {
            // A tab lifting off its frame: a small window with an arrow out
            // of its top-right corner.
            let (x, y) = (
                cell.x + (cell.w as i32 - 9) / 2,
                cell.y + (cell.h as i32 - 9) / 2,
            );
            canvas.stroke_rect(x, y + 3, 6, 6, color);
            canvas.line(x + 4, y + 4, x + 8, y, color);
            canvas.hline(x + 5, y, 4, color);
            canvas.vline(x + 8, y, 4, color);
        }
    }
}
