use api::display::{compositor_ops, AttachGrant, DamageNotify, DetachReplacedGrant, PixelFormat};
use ostd::syscall::{sys_grant_slice_with_len, sys_send};

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
/// The kernel resolves any Grant shared to the compositor, but reports neither
/// who registered it nor lets the compositor ask, so `reg_id` (a guessable
/// physical base) is not proof that the sender owns it. Two rules bound that:
/// a Grant another owner's surface already presents, stages or is retiring is
/// refused, and every refusal gets the same `\x00` reply as an id that does not
/// resolve, so probing ids reveals nothing. A Grant shared to the compositor
/// but not yet attached by its owner remains attachable by anyone; closing that
/// needs a kernel answer for the Grant owner.
fn attach_grant(buf: &[u8; 512], sender: usize, table: &mut SurfaceTable) {
    let Ok(frame) = <&[u8; 24]>::try_from(&buf[..24]) else {
        return;
    };
    let grant = AttachGrant::decode(frame);
    let reg_id = grant.reg_id as usize;
    let fmt = PixelFormat::from_u8(grant.fmt);
    let held_by_other = table
        .grant_holder(reg_id)
        .is_some_and(|holder| holder != sender);
    let geometry_ok = grant.width <= MAX_SURFACE_DIM && grant.height <= MAX_SURFACE_DIM;
    let attached = !held_by_other
        && geometry_ok
        && table
            .get(grant.cap as u64)
            .is_some_and(|surface| surface.owner == sender)
        && sys_grant_slice_with_len(reg_id).is_some_and(|(_, grant_len)| {
            (grant.width as usize)
                .checked_mul(grant.height as usize)
                .and_then(|pixels| pixels.checked_mul(fmt.bpp() as usize))
                .is_some_and(|required| required <= grant_len)
        })
        && table
            .get_mut(grant.cap as u64)
            .is_some_and(|surface| surface.attach_grant(reg_id, grant.width, grant.height, fmt));
    sys_send(sender, if attached { b"\x01" } else { b"\x00" });
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
