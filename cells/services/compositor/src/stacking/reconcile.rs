//! Converging a stack's hidden tabs on the size and state of the shown tab,
//! one configure per window at a time.

use api::display::{ConfigureKind, Rect, WindowState};
#[cfg(not(test))]
use ostd::syscall::{sys_try_send, SyscallResult};

use super::MAX_CONTENT;
use crate::surface_table::{FrameRegistry, SurfaceTable};
use crate::window_decoration;

/// Ask every hidden member of `frame_id` with nothing pending to take the
/// shown window's size (at most `maximum`, the screen's work area) and
/// maximized state. Waits while the shown window is
/// itself being configured, so a slow member cannot drag the frame to its
/// own size; a member that never answers only blocks itself.
pub(super) fn reconcile(
    frames: &FrameRegistry,
    table: &mut SurfaceTable,
    frame_id: u32,
    maximum: Rect,
) {
    let Some(shown) = frames.selected_cap(frame_id) else {
        return;
    };
    let Some(target) = table.get(shown).filter(|surface| !surface.is_configuring()) else {
        return;
    };
    let state = target.state();
    // Never ask a member for more than the screen offers, whatever size the
    // shown window's owner chose for itself.
    let rect = Rect {
        w: target.w.min(maximum.w.max(1)),
        h: target.h.min(maximum.h.max(1)),
        ..target.screen_rect()
    };
    for cap in frames
        .members(frame_id)
        .into_iter()
        .filter(|&cap| cap != shown)
    {
        match_model(table, cap, state, rect);
    }
}

/// [`reconcile`] the frame holding `cap`, if any, on a `screen` sized screen.
pub fn reconcile_frame_of(
    frames: &FrameRegistry,
    table: &mut SurfaceTable,
    cap: u64,
    screen: (u32, u32),
) {
    if let Some(frame) = frames.frame_of(cap) {
        reconcile(
            frames,
            table,
            frame,
            window_decoration::maximized_content(screen.0, screen.1),
        );
    }
}

/// Ask `cap`, if nothing is pending for it, for the `state` and size of
/// `model` (a frame's shown window): maximize, restore or resize.
pub(super) fn match_model(table: &mut SurfaceTable, cap: u64, state: WindowState, model: Rect) {
    let Some(member) = table.get(cap).filter(|member| !member.is_configuring()) else {
        return;
    };
    let rect = Rect {
        x: member.x,
        y: member.y,
        w: model.w.min(MAX_CONTENT),
        h: model.h.min(MAX_CONTENT),
    };
    let same_size = (member.w, member.h) == (rect.w, rect.h);
    let kind = match (state, member.state()) {
        (WindowState::Maximized, WindowState::Normal) => ConfigureKind::Maximize,
        (WindowState::Normal, WindowState::Maximized) => ConfigureKind::Restore,
        (WindowState::Normal, WindowState::Normal) if !same_size => ConfigureKind::Resize,
        (WindowState::Maximized, WindowState::Maximized) if !same_size => ConfigureKind::Maximize,
        _ => return,
    };
    propose(table, cap, kind, rect);
}

/// Send a configure proposal to `cap`'s owner; forget it if undeliverable.
pub(crate) fn propose(table: &mut SurfaceTable, cap: u64, kind: ConfigureKind, rect: Rect) {
    let Some(surface) = table.get_mut(cap) else {
        return;
    };
    if let Some(event) = surface.begin_configure(cap as u32, kind, rect) {
        let delivered = event
            .encode()
            .is_ok_and(|bytes| deliver(surface.owner, &bytes));
        if !delivered {
            surface.cancel_configure(event.serial);
        }
    }
}

#[cfg(not(test))]
pub(crate) fn deliver(owner: usize, bytes: &[u8]) -> bool {
    matches!(sys_try_send(owner, bytes), SyscallResult::Ok(0))
}

/// Host tests have no kernel: every owner takes the proposal.
#[cfg(test)]
pub(crate) fn deliver(_owner: usize, _bytes: &[u8]) -> bool {
    true
}
