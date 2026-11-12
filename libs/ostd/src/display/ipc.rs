//! Compositor lifecycle IPC transactions.
//!
//! Each request is one exact operation, so only the compositor's reply to it
//! is taken. Events the compositor sends while serving it (a configure's state
//! change, a palette announcement) are posted to the mailbox and stay there in
//! order for [`super::poll_surface_events`] and [`crate::input::poll_events`],
//! which route every compositor frame.

use api::display::{compositor_ops, AttachGrant, PixelFormat, SurfaceRole};
use types::{ViError, ViResult};

use crate::ipc::{call_bytes, post, IpcError};

/// Scheduler ticks (10 ms) one compositor request may take end to end: 5 s.
/// The compositor answers each request within one loop turn; longer means it
/// is stalled.
pub(super) const COMPOSITOR_CALL_TICKS: u64 = 500;

/// Budget of each teardown request `ViSurface::drop` makes: 1 s, so dropping a
/// surface against a stalled compositor holds the task only briefly.
pub(super) const TEARDOWN_CALL_TICKS: u64 = 100;

/// Post a one-way lifecycle frame; the compositor sends no reply.
pub(super) fn send_lifecycle_request(comp_tid: usize, frame: &[u8]) -> ViResult<()> {
    post(comp_tid, frame).map_err(|_| ViError::IO)
}

pub(super) fn create_surface(comp_tid: usize, w: u32, h: u32, role: SurfaceRole) -> ViResult<u32> {
    let mut req = [0u8; 10];
    req[0] = compositor_ops::CREATE_SURFACE;
    req[1..5].copy_from_slice(&w.to_le_bytes());
    req[5..9].copy_from_slice(&h.to_le_bytes());
    req[9] = role as u8;
    // The reply is the new cap as `u64` LE; zero means refused.
    let mut reply = [0u8; 8];
    let reply =
        call_bytes(comp_tid, &req, &mut reply, COMPOSITOR_CALL_TICKS).map_err(|_| ViError::IO)?;
    let cap = match reply {
        [a, b, c, d, ..] => u32::from_le_bytes([*a, *b, *c, *d]),
        _ => return Err(ViError::IO),
    };
    if cap == 0 {
        Err(ViError::IO)
    } else {
        Ok(cap)
    }
}

pub(super) fn surface_byte_len(width: u32, height: u32, fmt: PixelFormat) -> ViResult<usize> {
    let pixels = width.checked_mul(height).ok_or(ViError::InvalidArgument)?;
    let bytes = pixels
        .checked_mul(fmt.bpp())
        .ok_or(ViError::InvalidArgument)?;
    Ok(bytes as usize)
}

/// Send `request` and require the one-byte `expected_status` reply within `ticks`.
pub(super) fn request_status(
    comp_tid: usize,
    request: &[u8],
    expected_status: u8,
    ticks: u64,
) -> ViResult<()> {
    let mut reply = [0u8; 1];
    match call_bytes(comp_tid, request, &mut reply, ticks) {
        Ok([status]) if *status == expected_status => Ok(()),
        _ => Err(ViError::IO),
    }
}

pub(super) fn configure_ack(comp_tid: usize, cap: u32, serial: u32) -> ViResult<()> {
    let ack = api::display::ConfigureAck::new(cap, serial)
        .encode()
        .map_err(|_| ViError::InvalidInput)?;
    request_status(comp_tid, &ack, 0x01, COMPOSITOR_CALL_TICKS)
}

/// Detach the Grant `cap` presents, waiting at most `ticks` for the reply.
pub(super) fn detach_grant(comp_tid: usize, cap: u32, ticks: u64) -> ViResult<()> {
    let mut request = [0u8; 9];
    request[0] = compositor_ops::DETACH_GRANT;
    request[1..].copy_from_slice(&(cap as u64).to_le_bytes());
    request_status(comp_tid, &request, 0x01, ticks)
}

pub(super) fn detach_replaced_grant(comp_tid: usize, cap: u32, old_reg_id: usize) -> ViResult<()> {
    let request = api::display::DetachReplacedGrant::new(cap, old_reg_id as u64)
        .encode()
        .map_err(|_| ViError::InvalidInput)?;
    request_status(comp_tid, &request, 0x01, COMPOSITOR_CALL_TICKS)
}

pub(super) enum AttachGrantResult {
    Attached,
    Rejected,
    AmbiguousFailure,
}

/// Send `ATTACH_GRANT`, preserving the rejection/transport-failure distinction.
///
/// Only an outcome that proves the compositor holds no reference to the Grant
/// is `Rejected`: its `0x00` reply, or expiry before dispatch. Provider death,
/// expiry after dispatch and any other failure are `AmbiguousFailure`.
pub(super) fn stage_grant(
    comp_tid: usize,
    cap: u32,
    reg_id: usize,
    w: u32,
    h: u32,
    fmt: PixelFormat,
) -> AttachGrantResult {
    let ag = AttachGrant {
        opcode: compositor_ops::ATTACH_GRANT,
        fmt: fmt as u8,
        _pad: [0; 2],
        cap,
        reg_id: reg_id as u64,
        width: w,
        height: h,
    };
    let mut reply = [0u8; 1];
    match call_bytes(comp_tid, &ag.encode(), &mut reply, COMPOSITOR_CALL_TICKS) {
        Ok([0x01]) => AttachGrantResult::Attached,
        Ok([0x00]) | Err(IpcError::PreDispatchTimeout) => AttachGrantResult::Rejected,
        _ => AttachGrantResult::AmbiguousFailure,
    }
}

pub(super) fn attach_grant(
    comp_tid: usize,
    cap: u32,
    reg_id: usize,
    w: u32,
    h: u32,
    fmt: PixelFormat,
) -> ViResult<()> {
    match stage_grant(comp_tid, cap, reg_id, w, h, fmt) {
        AttachGrantResult::Attached => Ok(()),
        AttachGrantResult::Rejected | AttachGrantResult::AmbiguousFailure => Err(ViError::IO),
    }
}

/// Destroy surface `cap`, waiting at most `ticks` for the compositor's reply.
pub(super) fn destroy_surface(comp_tid: usize, cap: u32, ticks: u64) -> ViResult<()> {
    let mut req = [0u8; 9];
    req[0] = compositor_ops::DESTROY_SURFACE;
    req[1..9].copy_from_slice(&(cap as u64).to_le_bytes());
    request_status(comp_tid, &req, 0x00, ticks)
}

/// Unregister attempts [`release_grant`] makes before leaving the Grant to
/// task-exit reclamation.
const GRANT_RELEASE_ATTEMPTS: usize = 64;

/// Unregister `reg_id`, retrying while the compositor still leases it.
///
/// The compositor resolves a client Grant with a `GrantSlice` lease for each
/// blit; the lease ends at the compositor's next receive or its next send to
/// this client, and until then the kernel refuses `GrantUnregister`
/// (`PermissionDenied`, which this ABI reports as plain failure). Yielding lets
/// the compositor reach that boundary. The retry is bounded: a Grant that still
/// cannot be released (not ours, or a compositor stalled mid-frame) stays
/// registered and is reclaimed when this task exits.
pub(super) fn release_grant(reg_id: usize) -> bool {
    release_grant_with(
        reg_id,
        crate::syscall::sys_grant_unregister,
        crate::task::yield_now,
    )
}

fn release_grant_with(
    reg_id: usize,
    mut unregister: impl FnMut(usize) -> bool,
    mut yield_now: impl FnMut(),
) -> bool {
    for attempt in 1..=GRANT_RELEASE_ATTEMPTS {
        if unregister(reg_id) {
            return true;
        }
        if attempt < GRANT_RELEASE_ATTEMPTS {
            yield_now();
        }
    }
    false
}

#[cfg(test)]
mod release_grant_tests {
    use super::{release_grant_with, GRANT_RELEASE_ATTEMPTS};

    #[test]
    fn retries_a_leased_grant_until_the_lease_ends() {
        let mut refusals = 3;
        let mut yields = 0;
        let released = release_grant_with(
            7,
            |reg_id| {
                assert_eq!(reg_id, 7);
                if refusals == 0 {
                    return true;
                }
                refusals -= 1;
                false
            },
            || yields += 1,
        );
        assert!(released);
        assert_eq!(yields, 3, "one yield between consecutive attempts");
    }

    #[test]
    fn gives_up_after_a_bounded_number_of_attempts() {
        let mut attempts = 0;
        let mut yields = 0;
        let released = release_grant_with(
            7,
            |_| {
                attempts += 1;
                false
            },
            || yields += 1,
        );
        assert!(!released);
        assert_eq!(attempts, GRANT_RELEASE_ATTEMPTS);
        assert_eq!(yields, GRANT_RELEASE_ATTEMPTS - 1);
    }
}
