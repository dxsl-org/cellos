//! Scanout presented by the launcher that owns the guest's window.
//!
//! The VMM-owned scanout Grant is shared read-only with the bound launcher,
//! which copies damaged rectangles into its own window. Events are posted
//! (`VmmEvent`), never sent, so a launcher that stops receiving cannot park the
//! vCPU loop; damage is coalesced and retried at the next poll.

use super::{command, resource::ResourceTable};
use wire::vmm_session::VmmEvent;
use ostd::syscall::{sys_grant_share, sys_post, SyscallResult};

pub struct ClientScanout {
    client_tid: usize,
    width: u32,
    height: u32,
    /// Grant last shared with the launcher, and last announced to it (0: none).
    shared_reg: usize,
    announced_reg: usize,
    pending_damage: Option<command::Rect>,
}

impl ClientScanout {
    pub const fn new(client_tid: usize, width: u32, height: u32) -> Self {
        Self {
            client_tid,
            width,
            height,
            shared_reg: 0,
            announced_reg: 0,
            pending_damage: None,
        }
    }

    pub fn bring_up(&mut self, resources: &mut ResourceTable) {
        if resources.prepare_scanout(self.width, self.height).is_ok() {
            self.announce(resources);
        }
    }

    /// The guest display size changed; the next guest scanout uses it.
    pub fn resize(&mut self, resources: &mut ResourceTable, width: u32, height: u32) {
        self.width = width;
        self.height = height;
        self.pending_damage = None;
        self.bring_up(resources);
    }

    pub fn notify_damage(&mut self, resources: &ResourceTable, rect: command::Rect) {
        let Some((_, _, _, width, height)) = resources.scanout_grant() else {
            return;
        };
        let Some(rect) = super::clip_rect(rect, width, height) else {
            return;
        };
        self.pending_damage = Some(match self.pending_damage {
            Some(previous) => super::union_rect(previous, rect),
            None => rect,
        });
        self.poll(resources);
    }

    /// Announce a replaced Grant, then post coalesced damage.
    pub fn poll(&mut self, resources: &ResourceTable) {
        if !self.announce(resources) {
            return;
        }
        let Some(rect) = self.pending_damage else {
            return;
        };
        let event = VmmEvent::Damage {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: rect.height,
        };
        if self.post(&event) {
            self.pending_damage = None;
        }
    }

    pub fn exited(&self, code: u32) {
        let _ = self.post(&VmmEvent::Exited { code });
    }

    /// Share and announce the current scanout Grant if the launcher has not
    /// seen it; `false` while the announcement is still pending.
    fn announce(&mut self, resources: &ResourceTable) -> bool {
        let Some((reg_id, _, _, width, height)) = resources.scanout_grant() else {
            return false;
        };
        if reg_id == self.announced_reg {
            return true;
        }
        let event = VmmEvent::Scanout {
            grant: reg_id as u32,
            width,
            height,
        };
        if reg_id != self.shared_reg {
            if !sys_grant_share(reg_id, self.client_tid, 0) {
                return false;
            }
            self.shared_reg = reg_id;
        }
        if !self.post(&event) {
            return false;
        }
        self.announced_reg = reg_id;
        // The launcher repaints a new Grant whole.
        self.pending_damage = Some(command::Rect {
            x: 0,
            y: 0,
            width,
            height,
        });
        true
    }

    fn post(&self, event: &VmmEvent) -> bool {
        matches!(sys_post(self.client_tid, &event.encode()), SyscallResult::Ok(0))
    }
}
