//! Trusted compositor frame routing and lifecycle event polling.

use api::syscall::service;

use crate::syscall::{sys_lookup_service, SyscallResult};

use super::dispatcher;
use super::events::{SurfaceEvent, MAX_SURFACE_EVENTS};
use super::route_compositor_frame;

/// Poll up to `max` retained compositor lifecycle events.
///
/// The fixed-capacity result does not allocate. Only frames received from the
/// registered compositor are routed; forwarded input frames remain available to
/// [`crate::input::poll_events`].
pub fn poll_surface_events(max: usize) -> heapless::Vec<SurfaceEvent, MAX_SURFACE_EVENTS> {
    let limit = max.min(MAX_SURFACE_EVENTS);
    let mut events = heapless::Vec::new();
    while events.len() < limit {
        let Some(event) = dispatcher::take_surface() else {
            break;
        };
        let _ = events.push(event);
    }
    let Some(compositor_tid) = sys_lookup_service(service::COMPOSITOR) else {
        return events;
    };
    for _ in 0..limit.saturating_sub(events.len()) {
        if !drain_compositor_once(compositor_tid) {
            break;
        }
        while events.len() < limit {
            let Some(event) = dispatcher::take_surface() else {
                break;
            };
            let _ = events.push(event);
        }
    }
    events
}

/// Receive and route one frame from `compositor_tid`.
pub(crate) fn drain_compositor_once(compositor_tid: usize) -> bool {
    let mut buffer = [0u8; 72];
    match crate::syscall::sys_try_recv(compositor_tid, &mut buffer) {
        SyscallResult::Ok(sender) if sender == compositor_tid => {
            route_compositor_frame(&buffer);
            true
        }
        _ => false,
    }
}
