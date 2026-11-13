extern crate alloc;

use alloc::vec::Vec;
use wire::display::{
    compositor_events, ConfigureKind, Rect, WindowCloseRequest, WindowConfigure, WindowState,
    WindowStateChanged,
};
use core::cell::Cell;
use ostd::display::SurfaceEvent;

use super::{ClosePolicy, WindowEvent, WindowLifecycle};

fn configure(cap: u32) -> SurfaceEvent {
    SurfaceEvent::Configure(WindowConfigure {
        opcode: compositor_events::WINDOW_CONFIGURE,
        kind: ConfigureKind::Resize,
        _pad: [0; 2],
        cap,
        serial: 1,
        rect: Rect { x: 0, y: 0, w: 20, h: 10 },
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

/// Run `events` through a lifecycle whose surface accepts configures when
/// `resizes` and close responses when `delivers`; return what the app heard.
fn run(lifecycle: &mut WindowLifecycle, events: Vec<SurfaceEvent>, resizes: bool, delivers: bool) -> Vec<WindowEvent> {
    let answered = Cell::new(None);
    let mut heard = Vec::new();
    for event in events {
        let mut apply = |c: WindowConfigure| resizes.then_some((c.rect.w, c.rect.h));
        let mut respond = |_: WindowCloseRequest, accept: bool| {
            answered.set(Some(accept));
            delivers
        };
        lifecycle.handle_event(3, event, &mut apply, &mut respond, &mut |e| heard.push(e));
    }
    heard
}

#[test]
fn a_resize_reaches_the_app_only_once_the_surface_has_the_new_size() {
    let mut lifecycle = WindowLifecycle::new(ClosePolicy::Accept);
    let heard = run(&mut lifecycle, alloc::vec![configure(3)], true, true);
    assert_eq!(heard, [WindowEvent::Resized { width: 20, height: 10 }]);
    // A failed configure keeps the old buffer and layout: nothing to redo.
    assert!(run(&mut lifecycle, alloc::vec![configure(3)], false, true).is_empty());
}

#[test]
fn events_for_another_window_are_ignored() {
    let mut lifecycle = WindowLifecycle::new(ClosePolicy::Accept);
    let heard = run(&mut lifecycle, alloc::vec![configure(4), close(4), state(4, WindowState::Minimized)], true, true);
    assert!(heard.is_empty());
    assert!(!lifecycle.is_closed() && !lifecycle.is_minimized());
}

#[test]
fn minimize_and_restore_are_reported_once_each() {
    let mut lifecycle = WindowLifecycle::new(ClosePolicy::Accept);
    let events = alloc::vec![
        state(3, WindowState::Minimized),
        state(3, WindowState::Minimized),
        state(3, WindowState::Normal),
        state(3, WindowState::Maximized),
    ];
    let heard = run(&mut lifecycle, events, true, true);
    assert_eq!(heard, [WindowEvent::Minimized, WindowEvent::Restored]);
}

#[test]
fn close_policies_accept_reject_or_ask_the_app() {
    let mut accept = WindowLifecycle::new(ClosePolicy::Accept);
    assert_eq!(run(&mut accept, alloc::vec![close(3)], true, true), [WindowEvent::Closed]);
    assert!(accept.is_closed());

    let mut undelivered = WindowLifecycle::new(ClosePolicy::Accept);
    assert!(run(&mut undelivered, alloc::vec![close(3)], true, false).is_empty());
    assert!(!undelivered.is_closed(), "closed only once the compositor heard it");

    let mut reject = WindowLifecycle::new(ClosePolicy::Reject);
    assert!(run(&mut reject, alloc::vec![close(3)], true, true).is_empty());
    assert!(!reject.is_closed());

    let mut ask = WindowLifecycle::new(ClosePolicy::Ask);
    assert_eq!(run(&mut ask, alloc::vec![close(3)], true, true), [WindowEvent::CloseRequested { serial: 2 }]);
    assert!(!ask.is_closed(), "the app has not answered yet");
}
