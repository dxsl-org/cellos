//! A configure the owner is slow to answer never blocks closing or minimizing.

use wire::display::{
    CloseResponseAction, ConfigureKind, PixelFormat, Rect, SurfaceRole, WindowState,
};

use super::SurfaceState;

fn window() -> SurfaceState {
    SurfaceState::new(10, 20, 100, 80, 7, SurfaceRole::Interactive)
}

const BIGGER: Rect = Rect {
    x: 10,
    y: 20,
    w: 300,
    h: 200,
};

#[test]
fn a_window_closes_while_its_resize_is_unanswered() {
    let mut surface = window();
    let configure = surface
        .begin_configure(1, ConfigureKind::Resize, BIGGER)
        .unwrap();
    let (_, close, _) = surface
        .begin_close(1)
        .expect("close is not blocked by the configure");
    assert_eq!(surface.state(), WindowState::Closing);
    // The owner finally applies the resize while the close is asked: the
    // geometry commits, the window stays closing.
    assert!(surface.attach_grant(0x1000, BIGGER.w, BIGGER.h, PixelFormat::Bgra8888));
    surface.acknowledge_configure(1, configure.serial).unwrap();
    assert_eq!(
        (surface.w, surface.h, surface.state()),
        (300, 200, WindowState::Closing)
    );
    // Refused: back to the state the configure produced.
    surface
        .close_response(1, close.serial, CloseResponseAction::Reject)
        .unwrap();
    assert_eq!(surface.state(), WindowState::Normal);
}

#[test]
fn a_window_minimizes_while_its_maximize_is_unanswered_and_restores_maximized() {
    let mut surface = window();
    let configure = surface
        .begin_configure(1, ConfigureKind::Maximize, BIGGER)
        .unwrap();
    assert!(surface
        .request_state(1, wire::display::compositor_ops::MINIMIZE, BIGGER)
        .is_some());
    assert!(surface.attach_grant(0x1000, BIGGER.w, BIGGER.h, PixelFormat::Bgra8888));
    surface.acknowledge_configure(1, configure.serial).unwrap();
    assert_eq!(
        surface.state(),
        WindowState::Minimized,
        "the ack does not un-minimize"
    );
    assert!(surface
        .request_state(1, wire::display::compositor_ops::RESTORE, BIGGER)
        .is_some());
    assert_eq!(surface.state(), WindowState::Maximized);
}

#[test]
fn a_second_configure_still_waits_for_the_first() {
    let mut surface = window();
    surface
        .begin_configure(1, ConfigureKind::Resize, BIGGER)
        .unwrap();
    assert!(surface
        .begin_configure(1, ConfigureKind::Resize, BIGGER)
        .is_none());
    assert!(surface
        .request_state(1, wire::display::compositor_ops::MAXIMIZE, BIGGER)
        .is_none());
}
