use api::display::{compositor_ops, AttachGrant, DamageNotify, DetachReplacedGrant, PixelFormat};
use ostd::syscall::{sys_grant_slice_with_len_owned_by, sys_send};

use crate::framebuffer;
use crate::surface_table::{SurfaceTable, MAX_SURFACE_DIM};

pub(super) fn handle(buf: &[u8; 512], sender: usize, table: &mut SurfaceTable) {
    match buf[0] {
        compositor_ops::ATTACH_GRANT => attach_grant(buf, sender, table),
        compositor_ops::DETACH_REPLACED_GRANT => detach_replaced_grant(buf, sender, table),
        compositor_ops::DAMAGE_NOTIFY => damage_notify(buf, sender, table),
        compositor_ops::DETACH_GRANT => detach_grant(buf, sender, table),
        compositor_ops::GET_SCREEN_SIZE => screen_size(sender),
        _ => {}
    }
}

/// Bind a Grant to the sender's surface.
///
/// `reg_id` is a guessable physical base, so it is no proof that the sender
/// owns it: the kernel resolves the Grant only as the sender's own
/// (`GrantSlice` with an expected owner, H-27), which answers a Grant
/// registered by anyone else exactly as an unknown id. A Grant another owner's
/// surface already presents, stages or is retiring is refused before asking.
/// Every refusal gets the same `\x00` reply as an id that does not resolve, so
/// probing ids reveals nothing.
fn attach_grant(buf: &[u8; 512], sender: usize, table: &mut SurfaceTable) {
    let Ok(frame) = <&[u8; 24]>::try_from(&buf[..24]) else {
        return;
    };
    let grant = AttachGrant::decode(frame);
    let attached = admit_grant(&grant, sender, table, sys_grant_slice_with_len_owned_by);
    sys_send(sender, if attached { b"\x01" } else { b"\x00" });
}

/// Attach decision for [`attach_grant`]; `resolve(reg_id, owner_tid)` is the
/// owner-checked `GrantSlice` in production and a fake kernel in tests.
fn admit_grant(
    grant: &AttachGrant,
    sender: usize,
    table: &mut SurfaceTable,
    resolve: impl FnOnce(usize, usize) -> Option<(*mut u8, usize)>,
) -> bool {
    let reg_id = grant.reg_id as usize;
    let fmt = PixelFormat::from_u8(grant.fmt);
    let held_by_other = table
        .grant_holder(reg_id)
        .is_some_and(|holder| holder != sender);
    let geometry_ok = grant.width <= MAX_SURFACE_DIM && grant.height <= MAX_SURFACE_DIM;
    !held_by_other
        && geometry_ok
        && table
            .get(grant.cap as u64)
            .is_some_and(|surface| surface.owner == sender)
        && resolve(reg_id, sender).is_some_and(|(_, grant_len)| {
            (grant.width as usize)
                .checked_mul(grant.height as usize)
                .and_then(|pixels| pixels.checked_mul(fmt.bpp() as usize))
                .is_some_and(|required| required <= grant_len)
        })
        && table
            .get_mut(grant.cap as u64)
            .is_some_and(|surface| surface.attach_grant(reg_id, grant.width, grant.height, fmt))
}

fn detach_replaced_grant(buf: &[u8; 512], sender: usize, table: &mut SurfaceTable) {
    let Ok(frame) = DetachReplacedGrant::decode(&buf[..16]) else {
        sys_send(sender, b"\x00");
        return;
    };
    let Ok(reg_id) = usize::try_from(frame.reg_id) else {
        sys_send(sender, b"\x00");
        return;
    };
    let acknowledged = table
        .get_mut(frame.cap as u64)
        .filter(|surface| surface.owner == sender)
        .is_some_and(|surface| surface.detach_replaced_grant(reg_id));
    sys_send(sender, if acknowledged { b"\x01" } else { b"\x00" });
}

fn damage_notify(buf: &[u8; 512], sender: usize, table: &mut SurfaceTable) {
    if buf.len() < 24 {
        return;
    }
    let Ok(frame) = <&[u8; 24]>::try_from(&buf[..24]) else {
        return;
    };
    let damage = DamageNotify::decode(frame);
    if let Some(surface) = table
        .get_mut(damage.cap as u64)
        .filter(|surface| surface.owner == sender)
    {
        surface.damage = Some(match surface.damage {
            Some(existing) => existing.union(&damage.rect),
            None => damage.rect,
        });
    }
}

fn detach_grant(buf: &[u8; 512], sender: usize, table: &mut SurfaceTable) {
    if buf.len() < 9 {
        return;
    }
    let cap = u64::from_le_bytes(buf[1..9].try_into().unwrap());
    if let Some(surface) = table.get_mut(cap).filter(|surface| surface.owner == sender) {
        sys_send(
            sender,
            if surface.detach_grant() {
                b"\x01"
            } else {
                b"\x00"
            },
        );
    } else {
        sys_send(sender, b"\x00");
    }
}

fn screen_size(sender: usize) {
    let (width, height) = framebuffer::default_screen_size();
    let mut reply = [0u8; 8];
    reply[0..4].copy_from_slice(&width.to_le_bytes());
    reply[4..8].copy_from_slice(&height.to_le_bytes());
    sys_send(sender, &reply);
}

#[cfg(test)]
#[path = "grant_tests.rs"]
mod tests;
