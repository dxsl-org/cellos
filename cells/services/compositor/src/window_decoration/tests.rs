use super::*;

const CONTENT: Rect = Rect {
    x: 100,
    y: 80,
    w: 400,
    h: 300,
};
const SCREEN: (u32, u32) = (640, 480);

fn hit(x: i32, y: i32) -> Option<Hit> {
    hit_test(CONTENT, Tabs::SINGLE, x, y)
}

fn centre(r: Rect) -> (i32, i32) {
    (r.x + r.w as i32 / 2, r.y + r.h as i32 / 2)
}

#[test]
fn icon_title_and_content_are_reachable() {
    let (x, y) = centre(menu_button_rect(CONTENT, Tabs::SINGLE));
    assert!(hit(x, y) == Some(Hit::Menu));
    let tab = tab_rect(CONTENT, Tabs::SINGLE);
    // Everything right of the icon box up to the tab's end drags (no controls).
    let icon_end = end(menu_button_rect(CONTENT, Tabs::SINGLE).x, CONTROL as u32);
    for x in [icon_end, tab.x + tab.w as i32 / 2, end(tab.x, tab.w) - 2] {
        assert!(hit(x, tab.y + 10) == Some(Hit::Title), "x {x}");
    }
    assert!(hit(CONTENT.x, CONTENT.y) == Some(Hit::Content));
    assert!(hit(CONTENT.x + 399, CONTENT.y + 299) == Some(Hit::Content));
}

#[test]
fn window_menu_sits_below_the_icon_on_screen_and_hits_its_items() {
    let mut items = MenuItems::new();
    for action in [
        MenuAction::Maximize,
        MenuAction::Minimize,
        MenuAction::Close,
        MenuAction::CloseAll,
        MenuAction::StackOnto(9),
        MenuAction::StackOnto(10),
        MenuAction::Detach,
    ] {
        items.push(action);
    }
    let tabs = Tabs::stack(2, 1);
    let menu = menu_rect(CONTENT, tabs, &items, SCREEN.0, SCREEN.1);
    let button = menu_button_rect(CONTENT, tabs);
    assert!(menu.y > button.y + button.h as i32 && menu.x == button.x);
    let mut seen = alloc::vec::Vec::new();
    let mut gaps = 0;
    for y in menu.y..menu.y + menu.h as i32 {
        match menu_item_at(menu, &items, menu.x + 40, y) {
            Some(index) if seen.last() != Some(&index) => seen.push(index),
            None if !seen.is_empty() && seen.len() < items.len() => gaps += 1,
            _ => {}
        }
    }
    assert_eq!(
        seen,
        [0, 1, 2, 3, 4, 5, 6],
        "every item reachable, in order"
    );
    assert!(gaps > 0, "sections are set apart");
    assert_eq!(menu_item_at(menu, &items, menu.x - 1, menu.y + 10), None);
    // A window at the bottom-right corner keeps its menu on screen.
    let corner = Rect {
        x: 600,
        y: 470,
        w: 40,
        h: 10,
    };
    let shifted = menu_rect(corner, Tabs::SINGLE, &items, SCREEN.0, SCREEN.1);
    assert!(shifted.x + shifted.w as i32 <= SCREEN.0 as i32);
    assert!(shifted.y + shifted.h as i32 <= SCREEN.1 as i32);
}

#[test]
fn strip_beside_the_tab_passes_through() {
    let tab = tab_rect(CONTENT, Tabs::SINGLE);
    let x = end(tab.x, tab.w) + 20;
    assert!(hit(x, tab.y + 4).is_none());
    assert!(
        hit(x, tab.y - GRIP).is_none(),
        "grip band above the strip too"
    );
}

#[test]
fn margins_and_hairlines_resize_with_corner_grips() {
    let right = end(CONTENT.x, CONTENT.w);
    let bottom = end(CONTENT.y, CONTENT.h);
    let tab = tab_rect(CONTENT, Tabs::SINGLE);
    let cases = [
        (CONTENT.x - 1, CONTENT.y + 150, ResizeEdge::West),
        (CONTENT.x - GRIP, CONTENT.y + 150, ResizeEdge::West),
        (right + 3, CONTENT.y + 150, ResizeEdge::East),
        (CONTENT.x + 200, bottom, ResizeEdge::South),
        (CONTENT.x + 200, bottom + GRIP, ResizeEdge::South),
        (CONTENT.x + 50, tab.y - 2, ResizeEdge::North),
        (CONTENT.x + 300, CONTENT.y - 1, ResizeEdge::North),
        (CONTENT.x - 3, tab.y + 2, ResizeEdge::NorthWest),
        (CONTENT.x + 2, tab.y - 3, ResizeEdge::NorthWest),
        (right + 2, CONTENT.y + 2, ResizeEdge::NorthEast),
        (right - 2, CONTENT.y - 1, ResizeEdge::NorthEast),
        (CONTENT.x - 2, bottom - 3, ResizeEdge::SouthWest),
        (right + 1, bottom + 4, ResizeEdge::SouthEast),
    ];
    for (x, y, edge) in cases {
        assert!(hit(x, y) == Some(Hit::Resize(edge)), "({x}, {y})");
    }
    assert!(hit(CONTENT.x - BORDER - GRIP - 1, CONTENT.y).is_none());
    assert!(hit(CONTENT.x, bottom + BORDER + GRIP).is_none());
}

#[test]
fn maximized_content_keeps_the_whole_frame_on_screen() {
    let area = Rect {
        x: 0,
        y: 0,
        w: SCREEN.0,
        h: SCREEN.1 - 36,
    };
    let content = content_within(area);
    let [tab, body] = shapes(content, Tabs::SINGLE);
    assert_eq!((tab.x, tab.y), (0, 0));
    assert_eq!((body.x, body.w), (0, SCREEN.0));
    assert_eq!(body.y as u32 + body.h, area.h);
    let (x, y) = centre(menu_button_rect(content, Tabs::SINGLE));
    assert!(hit_test(content, Tabs::SINGLE, x, y) == Some(Hit::Menu));
}

/// Paint the frame of a `count`-tab stack, then blit an opaque content fill
/// the way `render` does.
fn compose(content: Rect, count: usize) -> alloc::vec::Vec<u8> {
    let (w, h) = SCREEN;
    let mut pixels = alloc::vec![0u8; (w * h * 4) as usize];
    let label = TabLabel {
        icon: ostd::display::linear::icons::LineIcon::Terminal,
        title: "",
        dimmed: false,
    };
    let labels = [label; 3];
    let style = Style {
        active: true,
        palette: chrome(api::display::AppearancePalette::Light),
        tabs: &labels[..count],
        selected: 0,
        offset: 0,
        menu_button: None,
    };
    paint(
        &mut pixels,
        w,
        h,
        content,
        Rect { x: 0, y: 0, w, h },
        &style,
        &mut TitleFont::new(),
    );
    let tabs = Tabs::stack(count, 0);
    for row in 0..content.h {
        let (start, end) = content_row_insets(content, tabs, row);
        for col in start..content.w - end {
            let at = (((content.y as u32 + row) * w + content.x as u32 + col) * 4) as usize;
            pixels[at..at + 4].copy_from_slice(&[1, 2, 3, 255]);
        }
    }
    pixels
}

#[test]
fn content_never_covers_the_rounded_outline() {
    for count in [1, 3] {
        let pixels = compose(CONTENT, count);
        let [_, body] = shapes(CONTENT, Tabs::stack(count, 0));
        let outline = chrome(api::display::AppearancePalette::Light)
            .active
            .outline;
        for row in 0..body.h {
            let ((l0, l1), (r0, r1)) = body.outline(row);
            let y = body.y as u32 + row;
            for col in (l0..l1).chain(r0..r1) {
                let at = ((y * SCREEN.0 + body.x as u32 + col) * 4) as usize;
                assert_eq!(
                    pixels[at..at + 4],
                    outline,
                    "outline at body ({col}, {row})"
                );
            }
            // Content reaches the outline: no background gap inside the frame.
            let (li, ri) = body.insets(row);
            for col in li..body.w - ri {
                let at = ((y * SCREEN.0 + body.x as u32 + col) * 4) as usize;
                assert_ne!(
                    pixels[at..at + 4],
                    [0; 4],
                    "{count} tabs: gap at body ({col}, {row})"
                );
            }
        }
    }
}

#[test]
fn paint_stays_inside_bounds_and_dirty() {
    let (w, h) = SCREEN;
    let mut pixels = alloc::vec![0u8; (w * h * 4) as usize];
    let dirty = Rect {
        x: 90,
        y: 40,
        w: 60,
        h: 400,
    };
    let icon = ostd::display::linear::icons::LineIcon::Folder;
    let tabs = [
        TabLabel {
            icon,
            title: "A long window title that will not fit the tab",
            dimmed: false,
        },
        TabLabel {
            icon,
            title: "Second",
            dimmed: false,
        },
        TabLabel {
            icon,
            title: "Third",
            dimmed: true,
        },
    ];
    let style = Style {
        active: false,
        palette: chrome(api::display::AppearancePalette::Dark),
        tabs: &tabs,
        selected: 1,
        offset: 0,
        menu_button: Some(true),
    };
    paint(
        &mut pixels,
        w,
        h,
        CONTENT,
        dirty,
        &style,
        &mut TitleFont::new(),
    );
    let outer = bounds(CONTENT);
    let mut painted = 0;
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            let at = ((y as u32 * w + x as u32) * 4) as usize;
            if pixels[at..at + 4] != [0; 4] {
                painted += 1;
                assert!(contains(outer, x, y) && contains(dirty, x, y), "({x}, {y})");
            }
        }
    }
    assert!(painted > 0);
}

#[test]
fn stacked_tabs_share_the_frame_width_without_gaps_or_overlap() {
    for count in 1..=6 {
        for width in [64, 300, 401, 1000] {
            let content = Rect {
                w: width,
                ..CONTENT
            };
            let tabs = Tabs::stack(count, count - 1);
            let strip = tab_rect(content, tabs);
            let mut next_x = strip.x;
            for index in 0..count {
                let slot = tab_slot(content, tabs, index);
                // Neighbours share one hairline column.
                let start = if index == 0 { slot.x } else { slot.x + 1 };
                assert_eq!(start, next_x, "{count} tabs, width {width}, tab {index}");
                next_x = end(slot.x, slot.w);
            }
            assert_eq!(
                next_x,
                end(strip.x, strip.w),
                "{count} tabs end with the strip"
            );
        }
    }
}

#[test]
fn a_stack_maps_each_tab_to_its_window_and_only_the_shown_tab_has_a_menu() {
    let tabs = Tabs::stack(3, 1);
    let shown = tab_slot(CONTENT, tabs, 1);
    let (bx, by) = centre(menu_button_rect(CONTENT, tabs));
    assert!(contains(shown, bx, by), "menu icon sits on the shown tab");
    assert_eq!(hit_test(CONTENT, tabs, bx, by), Some(Hit::Menu));
    for index in 0..3 {
        let slot = tab_slot(CONTENT, tabs, index);
        let expected = if index == 1 {
            Hit::Title
        } else {
            Hit::Tab(index)
        };
        // The icon box of another tab selects that tab, it opens no menu.
        let icon = icon_box(slot);
        let points = [(end(slot.x, slot.w) - 3, slot.y + 10), centre(icon)];
        for (x, y) in points {
            if index != 1 || !contains(icon, x, y) {
                assert_eq!(
                    hit_test(CONTENT, tabs, x, y),
                    Some(expected),
                    "tab {index} at ({x}, {y})"
                );
            }
        }
    }
}
