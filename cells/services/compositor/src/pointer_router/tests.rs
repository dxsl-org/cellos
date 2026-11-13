use wire::display::SurfaceRole;

use super::*;

fn window() -> (SurfaceTable, FrameRegistry, ZOrder, u64, Rect) {
    let mut table = SurfaceTable::new();
    let cap = table
        .create(100, 80, 300, 200, 7, SurfaceRole::Interactive)
        .unwrap();
    let mut frames = FrameRegistry::new();
    frames.adopt(cap, 7);
    let mut z_order = ZOrder::new();
    z_order.push(cap);
    let content = table.get(cap).unwrap().screen_rect();
    (table, frames, z_order, cap, content)
}

#[test]
fn hover_tracks_the_app_icon_and_its_open_menu() {
    let (table, frames, z_order, cap, content) = window();
    let button = window_decoration::menu_button_rect(content, window_decoration::Tabs::SINGLE);
    let (x, y) = (button.x + 10, button.y + 10);
    let mut router = PointerRouter::new();

    assert_eq!(
        router.update_hover(x, y, &table, &frames, &z_order),
        Some(button)
    );
    assert_eq!(
        router.menu_button_hover(),
        Some(MenuButtonHover { cap, open: false })
    );
    assert_eq!(
        router.update_hover(x + 1, y, &table, &frames, &z_order),
        None,
        "same icon: no repaint"
    );

    let items = window_menu::items_for(cap, &table, &frames, &z_order);
    let rect =
        window_decoration::menu_rect(content, window_decoration::Tabs::SINGLE, &items, 1280, 800);
    router.menu = Some(OpenMenu {
        cap,
        rect,
        items,
        hover: None,
    });
    assert_eq!(
        router.update_hover(x, y, &table, &frames, &z_order),
        Some(button)
    );
    assert!(router.menu_button_hover().is_some_and(|hover| hover.open));

    // Off the icon: no hover. A window drag shows none even over the icon.
    assert_eq!(
        router.update_hover(x, y + 60, &table, &frames, &z_order),
        Some(button)
    );
    router.capture = Some(Capture::Drag {
        cap,
        start_x: x,
        start_y: y,
        rect: content,
        mode: DragMode::Move,
    });
    assert_eq!(router.update_hover(x, y, &table, &frames, &z_order), None);
    assert_eq!(router.menu_button_hover(), None);
}

#[test]
fn a_closing_window_takes_its_menu_and_leaves_the_area_to_repaint() {
    let (_, _, _, cap, content) = window();
    let items = window_decoration::MenuItems::new();
    let rect =
        window_decoration::menu_rect(content, window_decoration::Tabs::SINGLE, &items, 1280, 800);
    let mut router = PointerRouter::new();
    router.menu = Some(OpenMenu {
        cap,
        rect,
        items,
        hover: Some(1),
    });
    router.forget(cap + 1);
    assert!(
        router.open_menu().is_some(),
        "another window's removal keeps it"
    );
    router.forget(cap);
    assert_eq!(router.open_menu(), None);
    assert_eq!(
        router.take_stale(),
        Some(window_decoration::menu_region(rect))
    );
    assert_eq!(router.take_stale(), None);
}

#[test]
fn double_click_needs_the_same_title_soon_and_close_by() {
    let mut router = PointerRouter::new();
    assert!(
        !input::is_double_click(&router, 5, 10, 10, 1_000),
        "first press"
    );
    router.last_title_press = Some((5, 1_000, 10, 10));
    assert!(input::is_double_click(
        &router,
        5,
        13,
        7,
        1_000 + DOUBLE_CLICK_MS
    ));
    assert!(
        !input::is_double_click(&router, 5, 10, 10, 1_001 + DOUBLE_CLICK_MS),
        "too late"
    );
    assert!(
        !input::is_double_click(&router, 5, 15, 10, 1_100),
        "moved away"
    );
    assert!(
        !input::is_double_click(&router, 6, 10, 10, 1_100),
        "another window"
    );
}

#[test]
fn pressing_a_stacked_tab_shows_and_focuses_its_window() {
    let (mut table, mut frames, mut z_order, a, content) = window();
    let b = table
        .create(400, 300, 300, 200, 8, SurfaceRole::Interactive)
        .unwrap();
    z_order.push(b);
    frames.adopt(b, 8);
    stacking::stack(&mut frames, &mut table, &mut z_order, b, a).unwrap();
    let centre = (content.x + 150, content.y + 100);
    assert_eq!(
        target::at(centre.0, centre.1, &table, &frames, &z_order).map(|t| t.cap),
        Some(b)
    );

    let tab = window_decoration::tab_slot(content, stacking::tabs_of(&frames, b), 0);
    let mut router = PointerRouter::new();
    let press = InputEvent::MouseButton {
        button: wire::input::MouseButton::Left,
        state: wire::input::KeyState::Pressed,
    };
    let maximum = window_decoration::maximized_content(1280, 800);
    let context = RouteContext::new(
        maximum,
        (1280, 800),
        0,
        &mut table,
        &mut frames,
        &mut z_order,
        |_| {},
    );
    router.route(press, tab.x + tab.w as i32 - 8, tab.y + 10, context);

    assert_eq!(frames.selected_cap(frames.frame_of(a).unwrap()), Some(a));
    assert_eq!(
        (router.focused_owner(), router.selected_cap()),
        (7, Some(a))
    );
    assert_eq!(
        router.capture.map(|_| ()),
        None,
        "picking a tab starts no drag"
    );
    let hit = target::at(centre.0, centre.1, &table, &frames, &z_order).map(|t| t.cap);
    assert_eq!(hit, Some(a), "the hidden window is never a pointer target");
}
