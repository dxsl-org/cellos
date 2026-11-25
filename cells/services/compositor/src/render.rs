//! Damage aggregation and ordered scanout composition.

use wire::display::{AppearancePalette, Rect, SurfaceRole};

use crate::framebuffer::ScreenFb;
use crate::pointer_look::PointerLook;
use crate::pointer_router::{DragFeedback, MenuButtonHover, OpenMenu};
use crate::surface_table::{frame::MAX_VIEWS_PER_FRAME, FrameRegistry, SurfaceTable};
use crate::window_decoration::{
    self, Control, MenuAction, MenuEntry, MenuGlyph, TabLabel, MAX_MENU_ITEMS,
};
use crate::z_order::ZOrder;
use ostd::display::linear::icons::LineIcon;

/// Compositor-owned state that decides how chrome looks this frame.
pub struct ChromeState {
    pub selected: Option<u64>,
    pub menu_button: Option<MenuButtonHover>,
    pub menu: Option<OpenMenu>,
    pub drag: Option<DragFeedback>,
    pub palette: AppearancePalette,
}

/// Repaint every surface intersecting the dirty region in stack order. A
/// window's shadow and frame (or a floating panel's shadow) go down first,
/// then its content clipped to its rounded shape, so the next surface up
/// covers all of it. Windows behind another tab of their stack are skipped:
/// the shown window paints the whole frame, every tab included. `cursor` is
/// the software pointer, `(look, x, y)`; `None` while the hardware cursor
/// shows it.
pub fn render_frame(
    fb: &mut ScreenFb,
    table: &mut SurfaceTable,
    frames: &FrameRegistry,
    z_order: &ZOrder,
    extra_dirty: Option<Rect>,
    chrome: &ChromeState,
    cursor: Option<(&PointerLook, i32, i32)>,
) -> Option<Rect> {
    let mut dirty = extra_dirty;
    for cap in z_order.iter_bottom_to_top() {
        let Some(surface) = table.get(cap) else {
            continue;
        };
        if !surface.is_shown() {
            continue;
        }
        let Some(damage) = surface.damage else {
            continue;
        };
        let content_damage = Rect {
            x: surface.x.saturating_add(damage.x),
            y: surface.y.saturating_add(damage.y),
            w: damage.w,
            h: damage.h,
        };
        let screen_damage = if surface.is_window_managed() {
            window_decoration::bounds(surface.screen_rect())
        } else {
            content_damage
        };
        dirty = Some(dirty.map_or(screen_damage, |current| current.union(&screen_damage)));
    }
    let dirty = dirty?;
    fb.clear_rect(dirty);
    let palette = window_decoration::chrome(chrome.palette);
    for cap in z_order.iter_bottom_to_top() {
        let Some(surface) = table.get(cap).filter(|surface| surface.is_shown()) else {
            continue;
        };
        let rect = surface.screen_rect();
        let tabs = crate::stacking::tabs_of(frames, cap);
        if surface.is_window_managed() && window_decoration::bounds(rect).intersects(&dirty) {
            paint_chrome(fb, table, frames, cap, dirty, chrome);
        }
        let Some(surface) = table.get_mut(cap) else {
            continue;
        };
        if surface.is_window_managed() {
            if rect.intersects(&dirty) {
                fb.blit_surface_clipped(surface, dirty, |row| {
                    window_decoration::content_row_insets(rect, tabs, row)
                });
            }
        } else if surface.role == SurfaceRole::Panel {
            let (shape, floating) = window_decoration::panel_shape(rect, fb.width, fb.height);
            if floating && window_decoration::panel_region(rect).intersects(&dirty) {
                fb.paint_panel_shadow(&shape, dirty, &palette.shadow);
            }
            if rect.intersects(&dirty) {
                fb.blit_surface_clipped(surface, dirty, |row| shape.insets(row));
            }
        } else if rect.intersects(&dirty) {
            fb.blit_surface_clipped(surface, dirty, |_| (0, 0));
        }
    }
    // A Win+drag's ghost and target outline float above every window.
    if let Some(drag) = chrome.drag.filter(|drag| drag.area().intersects(&dirty)) {
        fb.paint_drag_feedback(
            &drag,
            dirty,
            window_decoration::chrome(chrome.palette).accent,
        );
    }
    // The window menu floats above every surface.
    if let Some(menu) = chrome.menu {
        if window_decoration::menu_region(menu.rect).intersects(&dirty) {
            paint_menu(fb, table, frames, &menu, dirty, chrome);
        }
    }
    for cap in z_order.iter_bottom_to_top() {
        if let Some(surface) = table.get_mut(cap) {
            surface.clear_damage();
        }
    }
    if let Some((look, x, y)) = cursor {
        fb.composite_cursor(look, x, y, dirty);
    }
    fb.flush_rect(dirty);
    Some(dirty)
}

/// Paint the frame of the shown window `cap`: shadow, body and every tab of
/// its stack, in tab order.
fn paint_chrome(
    fb: &mut ScreenFb,
    table: &SurfaceTable,
    frames: &FrameRegistry,
    cap: u64,
    dirty: Rect,
    chrome: &ChromeState,
) {
    let Some(surface) = table.get(cap) else {
        return;
    };
    let mut labels = [TabLabel {
        icon: surface.icon(),
        title: "",
        dimmed: false,
    }; MAX_VIEWS_PER_FRAME];
    let mut count = 0;
    let mut selected = 0;
    let members = frames
        .frame_of(cap)
        .map(|frame| frames.members(frame))
        .unwrap_or_default();
    for member in members
        .iter()
        .filter_map(|&member| table.get(member).map(|s| (member, s)))
    {
        if count == labels.len() {
            break;
        }
        if member.0 == cap {
            selected = count;
        }
        labels[count] = label(member.1);
        count += 1;
    }
    if count == 0 {
        labels[0] = label(surface);
        count = 1;
    }
    let menu_open = chrome.menu.is_some_and(|menu| menu.cap == cap);
    let hovered = chrome.menu_button.is_some_and(|hover| hover.cap == cap);
    let style = window_decoration::Style {
        active: Some(cap) == chrome.selected,
        palette: window_decoration::chrome(chrome.palette),
        tabs: &labels[..count],
        selected,
        offset: crate::stacking::tabs_of(frames, cap).offset,
        menu_button: (hovered || menu_open).then_some(menu_open),
    };
    fb.paint_window_decoration(surface.screen_rect(), dirty, &style);
}

fn label(surface: &crate::surface_table::SurfaceState) -> TabLabel<'_> {
    TabLabel {
        icon: surface.icon(),
        title: surface.title(),
        dimmed: surface.state() == wire::display::WindowState::Closing,
    }
}

/// Paint the open window menu: one entry per item, labelled from the
/// windows it names as they are now.
fn paint_menu(
    fb: &mut ScreenFb,
    table: &SurfaceTable,
    frames: &FrameRegistry,
    menu: &OpenMenu,
    dirty: Rect,
    chrome: &ChromeState,
) {
    let maximized = table
        .get(menu.cap)
        .is_some_and(|s| s.state() == wire::display::WindowState::Maximized);
    let stacked = crate::stacking::tabs_of(frames, menu.cap).count > 1;
    let blank = MenuEntry {
        glyph: MenuGlyph::Detach,
        prefix: "",
        label: "",
    };
    let mut entries = [blank; MAX_MENU_ITEMS];
    let mut count = 0;
    for action in menu.items.iter() {
        let control = |control, label| MenuEntry {
            glyph: MenuGlyph::Control(control, maximized),
            prefix: "",
            label,
        };
        entries[count] = match action {
            MenuAction::Maximize => control(
                Control::Maximize,
                if maximized { "Restore" } else { "Maximize" },
            ),
            MenuAction::Minimize => control(Control::Minimize, "Minimize"),
            MenuAction::StackOnto(target) => {
                let (icon, title) = table
                    .get(target)
                    .map_or((LineIcon::Application, ""), |s| (s.icon(), s.title()));
                MenuEntry {
                    glyph: MenuGlyph::Icon(icon),
                    prefix: "Stack onto",
                    label: if title.is_empty() { "window" } else { title },
                }
            }
            MenuAction::Untile => MenuEntry {
                glyph: MenuGlyph::Detach,
                prefix: "",
                label: "Untile",
            },
            MenuAction::Detach => MenuEntry {
                glyph: MenuGlyph::Detach,
                prefix: "",
                label: "Detach tab",
            },
            MenuAction::Close => {
                control(Control::Close, if stacked { "Close tab" } else { "Close" })
            }
            MenuAction::CloseAll => control(Control::Close, "Close all tabs"),
            MenuAction::CloseGroup => control(Control::Close, "Close group"),
        };
        count += 1;
    }
    fb.paint_window_menu(
        menu,
        dirty,
        &entries[..count],
        window_decoration::chrome(chrome.palette),
    );
}
