//! Compositor lifecycle IPC transactions.

use api::display::{compositor_events, compositor_ops, AttachGrant, PixelFormat, SurfaceRole};
use types::{ViError, ViResult};

use crate::syscall::{sys_recv, sys_send, SyscallResult};

use super::route_compositor_frame;

pub(super) fn send_lifecycle_request(comp_tid: usize, frame: &[u8]) -> ViResult<()> {
    match sys_send(comp_tid, frame) {
        SyscallResult::Ok(_) => Ok(()),
        SyscallResult::Err(_) => Err(ViError::IO),
    }
}

pub(super) fn create_surface(comp_tid: usize, w: u32, h: u32, role: SurfaceRole) -> ViResult<u32> {
    let mut req = [0u8; 10];
    req[0] = compositor_ops::CREATE_SURFACE;
    req[1..5].copy_from_slice(&w.to_le_bytes());
    req[5..9].copy_from_slice(&h.to_le_bytes());
    req[9] = role as u8;
    sys_send(comp_tid, &req);

    let mut resp = [0u8; 8];
    match sys_recv(comp_tid, &mut resp) {
        SyscallResult::Ok(sender) if sender == comp_tid => {
            let cap = u32::from_le_bytes([resp[0], resp[1], resp[2], resp[3]]);
            if cap == 0 {
                Err(ViError::IO)
            } else {
                Ok(cap)
            }
        }
        _ => Err(ViError::IO),
    }
}

pub(super) fn surface_byte_len(width: u32, height: u32, fmt: PixelFormat) -> ViResult<usize> {
    let pixels = width.checked_mul(height).ok_or(ViError::InvalidArgument)?;
    let bytes = pixels
        .checked_mul(fmt.bpp())
        .ok_or(ViError::InvalidArgument)?;
    Ok(bytes as usize)
}

/// Receive a status while routing interleaved lifecycle and input frames.
pub(super) fn receive_status(comp_tid: usize, expected_status: u8) -> ViResult<()> {
    loop {
        let mut frame = [0u8; 72];
        match sys_recv(comp_tid, &mut frame) {
            SyscallResult::Ok(sender) if sender == comp_tid => match frame[0] {
                api::input::INPUT_EVENT_OPCODE
                | compositor_events::WINDOW_CONFIGURE
                | compositor_events::WINDOW_CLOSE_REQUEST
                | compositor_events::WINDOW_STATE_CHANGED => route_compositor_frame(&frame),
                status if status == expected_status => return Ok(()),
                _ => return Err(ViError::IO),
            },
            _ => return Err(ViError::IO),
        }
    }
}

pub(super) fn configure_ack(comp_tid: usize, cap: u32, serial: u32) -> ViResult<()> {
    let ack = api::display::ConfigureAck::new(cap, serial)
        .encode()
        .map_err(|_| ViError::InvalidInput)?;
    sys_send(comp_tid, &ack);
    receive_status(comp_tid, 0x01)
}

pub(super) fn detach_grant(comp_tid: usize, cap: u32) -> ViResult<()> {
    let mut request = [0u8; 9];
    request[0] = compositor_ops::DETACH_GRANT;
    request[1..].copy_from_slice(&(cap as u64).to_le_bytes());
    sys_send(comp_tid, &request);
    receive_status(comp_tid, 0x01)
}

pub(super) fn detach_replaced_grant(comp_tid: usize, cap: u32, old_reg_id: usize) -> ViResult<()> {
    let request = api::display::DetachReplacedGrant::new(cap, old_reg_id as u64)
        .encode()
        .map_err(|_| ViError::InvalidInput)?;
    sys_send(comp_tid, &request);
    receive_status(comp_tid, 0x01)
}

pub(super) enum AttachGrantResult {
    Attached,
    Rejected,
    AmbiguousFailure,
}

/// Send `ATTACH_GRANT`, preserving the rejection/transport-failure distinction.
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
    if !matches!(sys_send(comp_tid, &ag.encode()), SyscallResult::Ok(_)) {
        return AttachGrantResult::AmbiguousFailure;
    }
    loop {
        let mut frame = [0u8; 72];
        match sys_recv(comp_tid, &mut frame) {
            SyscallResult::Ok(sender) if sender == comp_tid => match frame[0] {
                api::input::INPUT_EVENT_OPCODE
                | compositor_events::WINDOW_CONFIGURE
                | compositor_events::WINDOW_CLOSE_REQUEST
                | compositor_events::WINDOW_STATE_CHANGED => route_compositor_frame(&frame),
                0x01 => return AttachGrantResult::Attached,
                0x00 => return AttachGrantResult::Rejected,
                _ => return AttachGrantResult::AmbiguousFailure,
            },
            _ => return AttachGrantResult::AmbiguousFailure,
        }
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

pub(super) fn destroy_surface(comp_tid: usize, cap: u32) -> ViResult<()> {
    let mut req = [0u8; 9];
    req[0] = compositor_ops::DESTROY_SURFACE;
    req[1..9].copy_from_slice(&(cap as u64).to_le_bytes());
    sys_send(comp_tid, &req);
    receive_status(comp_tid, 0x00)
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
