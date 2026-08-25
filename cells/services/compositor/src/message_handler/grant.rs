use api::display::{compositor_ops, AttachGrant, DamageNotify, DetachReplacedGrant, PixelFormat};
use ostd::syscall::{sys_grant_slice_with_len, sys_send};

use crate::framebuffer;
use crate::surface_table::SurfaceTable;

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

fn attach_grant(buf: &[u8; 512], sender: usize, table: &mut SurfaceTable) {
    if buf.len() < 24 {
        return;
    }
    let Ok(frame) = <&[u8; 24]>::try_from(&buf[..24]) else {
        return;
    };
    let grant = AttachGrant::decode(frame);
    let cap = grant.cap as u64;
    if let Some(surface) = table.get_mut(cap) {
        if surface.owner != sender {
            sys_send(sender, b"\x00");
            return;
        }
        match sys_grant_slice_with_len(grant.reg_id as usize) {
            Some((ptr, grant_len))
                if (grant.width as usize)
                    .checked_mul(grant.height as usize)
                    .and_then(|pixels| {
                        pixels.checked_mul(PixelFormat::from_u8(grant.fmt).bpp() as usize)
                    })
                    .is_some_and(|required| required <= grant_len) =>
            {
                if surface.attach_grant(
                    ptr as *const u8,
                    grant.reg_id as usize,
                    grant.width,
                    grant.height,
                    PixelFormat::from_u8(grant.fmt),
                ) {
                    sys_send(sender, b"\x01");
                } else {
                    sys_send(sender, b"\x00");
                }
            }
            _ => {
                sys_send(sender, b"\x00");
            }
        }
    } else {
        sys_send(sender, b"\x00");
    }
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
    if let Some(surface) = table.get_mut(damage.cap as u64) {
        if surface.owner == sender {
            surface.damage = Some(match surface.damage {
                Some(existing) => existing.union(&damage.rect),
                None => damage.rect,
            });
        }
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
