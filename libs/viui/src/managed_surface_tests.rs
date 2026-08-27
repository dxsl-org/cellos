use api::display::{
    compositor_events, ConfigureKind, Rect, WindowCloseRequest, WindowConfigure, WindowState,
    WindowStateChanged,
};
use core::cell::Cell;
use ostd::display::SurfaceEvent;

use super::{classify_event, ClosePolicy, LifecycleAction, LifecycleController};

fn configure(cap: u32) -> SurfaceEvent {
    SurfaceEvent::Configure(WindowConfigure {
        opcode: compositor_events::WINDOW_CONFIGURE,
        kind: ConfigureKind::Resize,
        _pad: [0; 2],
        cap,
        serial: 1,
        rect: Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 10,
        },
    })
}

fn state(cap: u32, state: WindowState) -> SurfaceEvent {
    SurfaceEvent::StateChanged(WindowStateChanged {
        opcode: compositor_events::WINDOW_STATE_CHANGED,
        state,
        _pad: [0; 2],
        cap,
        serial: 1,
    })
}

fn close(cap: u32) -> SurfaceEvent {
    SurfaceEvent::CloseRequest(WindowCloseRequest {
        opcode: compositor_events::WINDOW_CLOSE_REQUEST,
        _pad: [0; 3],
        cap,
        serial: 2,
    })
}

#[test]
fn lifecycle_events_are_capability_scoped() {
    assert!(matches!(
        classify_event(7, configure(7)),
        LifecycleAction::Configure(_)
    ));
    assert!(matches!(
        classify_event(8, configure(7)),
        LifecycleAction::Ignore
    ));
}

#[test]
fn lifecycle_controller_preserves_failure_and_state_transitions() {
    let mut controller = LifecycleController::new();
    let apply_calls = Cell::new(0);
    let response = Cell::new(None);
    let mut apply = |_| {
        apply_calls.set(apply_calls.get() + 1);
        false
    };
    let mut respond = |_, accept| {
        response.set(Some(accept));
        true
    };
    assert!(!controller.handle_event(3, configure(3), &mut apply, &mut respond));
    assert_eq!(apply_calls.get(), 1);
    assert!(!controller.handle_event(
        3,
        state(3, WindowState::Minimized),
        &mut apply,
        &mut respond
    ));
    assert!(controller.minimized);
    assert!(controller.handle_event(3, state(3, WindowState::Normal), &mut apply, &mut respond));
    assert!(!controller.minimized);
    assert!(!controller.handle_event(3, close(3), &mut apply, &mut respond));
    assert_eq!(response.get(), Some(false));
    assert!(!controller.closed);
}

#[test]
fn accepted_close_requires_a_successful_response() {
    let mut controller = LifecycleController::new();
    controller.close_policy = ClosePolicy::Accept;
    let mut apply = |_| true;
    let mut failing_response = |_, _| false;
    assert!(!controller.handle_event(3, close(3), &mut apply, &mut failing_response));
    assert!(!controller.closed);
    let mut successful_response = |_, accept| accept;
    assert!(!controller.handle_event(3, close(3), &mut apply, &mut successful_response));
    assert!(controller.closed);
}

#[test]
fn matching_state_and_close_events_are_classified() {
    assert!(matches!(
        classify_event(3, state(3, WindowState::Minimized)),
        LifecycleAction::State(true)
    ));
    assert!(matches!(
        classify_event(3, close(3)),
        LifecycleAction::Close(_)
    ));
}
