//! Non-blocking compositor bridge for the VMM-owned scanout Grant.

use super::{command, resource::ResourceTable};
use api::display::{compositor_ops, AttachGrant, DamageNotify, PixelFormat, Rect};
use ostd::io::println;
use ostd::syscall::{sys_grant_share, sys_recv_timeout, sys_try_send, sys_yield, SyscallResult};
pub struct ScanoutBridge {
    compositor_tid: usize,
    surface_cap: u32,
    pending_destroy_cap: u32,
    width: u32,
    height: u32,
    pending_damage: Option<command::Rect>,
}

impl ScanoutBridge {
    pub const fn new(compositor_tid: usize, width: u32, height: u32) -> Self {
        Self {
            compositor_tid,
            surface_cap: 0,
            pending_destroy_cap: 0,
            width,
            height,
            pending_damage: None,
        }
    }
    pub fn bring_up(&mut self, resources: &mut ResourceTable) {
        if self.compositor_tid == 0 || self.pending_destroy_cap != 0 {
            return;
        }
        if self.surface_cap != 0 {
            let mut destroy = [0u8; 9];
            destroy[0] = compositor_ops::DESTROY_SURFACE;
            destroy[1..9].copy_from_slice(&(self.surface_cap as u64).to_le_bytes());
            if self.request(&destroy).is_some_and(|reply| reply[0] == 0) {
                self.surface_cap = 0;
                resources.teardown_scanout();
            } else {
                return;
            }
        }
        if resources.prepare_scanout(self.width, self.height).is_err() {
            return;
        }
        let Some((reg_id, _, _, _, _)) = resources.scanout_grant() else {
            return;
        };
        if !sys_grant_share(reg_id, self.compositor_tid, 0) {
            return;
        }

        let mut create = [0u8; 9];
        create[0] = compositor_ops::CREATE_SURFACE;
        create[1..5].copy_from_slice(&self.width.to_le_bytes());
        create[5..9].copy_from_slice(&self.height.to_le_bytes());
        let Some(reply) = self.request(&create) else {
            return;
        };
        let cap = u32::from_le_bytes(reply[..4].try_into().unwrap_or([0; 4]));
        if cap == 0 {
            return;
        }

        let attach = AttachGrant {
            opcode: compositor_ops::ATTACH_GRANT,
            fmt: PixelFormat::Bgra8888 as u8,
            _pad: [0; 2],
            cap,
            reg_id: reg_id as u64,
            width: self.width,
            height: self.height,
        };
        let Some(reply) = self.request(&attach.encode()) else {
            return;
        };
        if reply.first().copied() == Some(1) {
            self.surface_cap = cap;
        } else {
            let mut destroy = [0u8; 9];
            destroy[0] = compositor_ops::DESTROY_SURFACE;
            destroy[1..9].copy_from_slice(&(cap as u64).to_le_bytes());
            let _ = self.request(&destroy);
        }
    }
    pub fn reconnect(&mut self, compositor_tid: usize, resources: &mut ResourceTable) {
        if compositor_tid == 0 {
            return;
        }
        if compositor_tid != self.compositor_tid {
            self.compositor_tid = compositor_tid;
            self.surface_cap = 0;
            self.pending_destroy_cap = 0;
            self.pending_damage = None;
        }
        self.retry_teardown(resources);
        if self.surface_cap == 0 {
            self.bring_up(resources);
        }
    }
    pub fn notify_damage(&mut self, rect: command::Rect) {
        if self.surface_cap == 0 {
            return;
        }
        let rect = command::Rect {
            x: rect.x,
            y: rect.y,
            width: rect.width.min(self.width.saturating_sub(rect.x)),
            height: rect.height.min(self.height.saturating_sub(rect.y)),
        };
        if rect.width == 0 || rect.height == 0 {
            return;
        }
        self.pending_damage = Some(match self.pending_damage {
            Some(previous) => union_rect(previous, rect),
            None => rect,
        });
        self.poll_damage();
    }
    pub fn poll_damage(&mut self) {
        let Some(rect) = self.pending_damage else {
            return;
        };
        let rect = Rect {
            x: rect.x as i32,
            y: rect.y as i32,
            w: rect.width,
            h: rect.height,
        };
        if rect.w == 0 || rect.h == 0 {
            return;
        }
        let message = DamageNotify {
            opcode: compositor_ops::DAMAGE_NOTIFY,
            _pad: [0; 3],
            cap: self.surface_cap,
            rect,
        };
        if matches!(
            sys_try_send(self.compositor_tid, &message.encode()),
            SyscallResult::Ok(0)
        ) {
            self.pending_damage = None;
        }
    }
    pub fn reset(&mut self, resources: &mut ResourceTable) {
        let needs_destroy = self.surface_cap != 0 || self.pending_destroy_cap != 0;
        if self.surface_cap != 0 {
            self.pending_destroy_cap = self.surface_cap;
            self.surface_cap = 0;
        }
        self.pending_damage = None;
        self.retry_teardown(resources);
        if self.pending_destroy_cap != 0 {
            println("[hv-gpu] scanout teardown deferred");
        } else if !needs_destroy {
            resources.teardown_scanout();
            println("[hv-gpu] scanout teardown ok");
        }
        resources.reset();
    }
    fn retry_teardown(&mut self, resources: &mut ResourceTable) {
        if self.pending_destroy_cap == 0 {
            return;
        }
        let mut destroy = [0u8; 9];
        destroy[0] = compositor_ops::DESTROY_SURFACE;
        destroy[1..9].copy_from_slice(&(self.pending_destroy_cap as u64).to_le_bytes());
        if self.request(&destroy).is_some_and(|reply| reply[0] == 0) {
            self.pending_destroy_cap = 0;
            resources.teardown_scanout();
            println("[hv-gpu] scanout teardown ok");
        }
    }
    fn request(&self, message: &[u8]) -> Option<[u8; 32]> {
        for _ in 0..100 {
            if matches!(
                sys_try_send(self.compositor_tid, message),
                SyscallResult::Ok(0)
            ) {
                let mut reply = [0u8; 32];
                return match sys_recv_timeout(self.compositor_tid, &mut reply, 20) {
                    SyscallResult::Ok(sender) if sender == self.compositor_tid => Some(reply),
                    _ => None,
                };
            }
            sys_yield();
        }
        None
    }
}
fn union_rect(a: command::Rect, b: command::Rect) -> command::Rect {
    let left = a.x.min(b.x);
    let top = a.y.min(b.y);
    let right = a.x.saturating_add(a.width).max(b.x.saturating_add(b.width));
    let bottom =
        a.y.saturating_add(a.height)
            .max(b.y.saturating_add(b.height));
    command::Rect {
        x: left,
        y: top,
        width: right.saturating_sub(left),
        height: bottom.saturating_sub(top),
    }
}
