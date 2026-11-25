//! The Linux guest behind the window: this launcher's own hypervisor Cell and
//! the `vmm_session` protocol to it.

use alloc::collections::VecDeque;
use alloc::string::ToString;

use ostd::syscall::{sys_post, sys_recv_timeout, sys_try_recv, SyscallResult};
use wire::vmm_session::{
    VmmClipboardRequest, VmmEvent, VmmInputForwardRequest, VmmLaunchReply, VmmLaunchRequest,
    VmmResizeRequest, VMM_OP_TERMINATE_APP, VMM_STATUS_OK,
};

use crate::input_bridge::GuestEvent;

/// How long the hypervisor may take to answer LAUNCH (10 ms ticks).
const LAUNCH_REPLY_TICKS: u64 = 500;
/// Forwarded input events held while the hypervisor's mailbox is full.
const OUTBOX_CAPACITY: usize = 512;

pub struct Guest {
    hypervisor_tid: usize,
    session_id: u32,
}

/// Why the guest could not be started; shown in the window as is.
pub type StartError = &'static str;

impl Guest {
    /// Spawn `/bin/hypervisor app` and bind it to this launcher with the
    /// window's content size.
    pub fn start(width: u32, height: u32) -> Result<Self, StartError> {
        if !ostd::set_spawn_argv(&["app".to_string()]) {
            return Err("cannot pass arguments to the hypervisor");
        }
        let hypervisor_tid = match ostd::syscall::sys_spawn_from_path_raw("/bin/hypervisor") {
            SyscallResult::Ok(tid) if tid != 0 => tid,
            _ => return Err("the hypervisor Cell could not be started"),
        };
        let mut request = [0u8; VmmLaunchRequest::LEN];
        VmmLaunchRequest::new("chrome", width, height)
            .encode(&mut request)
            .map_err(|_| "cannot encode the launch request")?;
        if !matches!(sys_post(hypervisor_tid, &request), SyscallResult::Ok(0)) {
            return Err("the hypervisor did not accept the launch request");
        }
        let mut reply = [0u8; VmmLaunchReply::LEN];
        let replied = matches!(
            sys_recv_timeout(hypervisor_tid, &mut reply, LAUNCH_REPLY_TICKS),
            SyscallResult::Ok(sender) if sender == hypervisor_tid
        );
        let reply = VmmLaunchReply::decode(&reply)
            .ok()
            .filter(|_| replied)
            .ok_or("the hypervisor did not answer")?;
        if reply.status != VMM_STATUS_OK {
            return Err("the hypervisor refused the window size");
        }
        Ok(Self {
            hypervisor_tid,
            session_id: reply.session_id,
        })
    }

    pub fn hypervisor_tid(&self) -> usize {
        self.hypervisor_tid
    }

    /// Post one evdev event; `false` when the hypervisor's mailbox is full.
    fn forward(&self, (event_type, code, value): GuestEvent) -> bool {
        let request = VmmInputForwardRequest::new(self.session_id, event_type, code, value);
        let mut buf = [0u8; VmmInputForwardRequest::LEN];
        if request.encode(&mut buf).is_err() {
            return true;
        }
        matches!(sys_post(self.hypervisor_tid, &buf), SyscallResult::Ok(0))
    }

    /// Ask the guest to use a new display size (the window was resized).
    pub fn resize(&self, width: u32, height: u32) {
        let request = VmmResizeRequest {
            session_id: self.session_id,
            width,
            height,
        };
        let _ = sys_post(self.hypervisor_tid, &request.encode());
    }

    /// Hand the guest `len` bytes of clipboard text at the start of `grant`
    /// (this launcher's, shared read-only with the hypervisor); `false` when
    /// the hypervisor's mailbox is full.
    pub fn send_clipboard(&self, grant: u32, len: u32) -> bool {
        let request = VmmClipboardRequest {
            session_id: self.session_id,
            grant,
            len,
        };
        matches!(
            sys_post(self.hypervisor_tid, &request.encode()),
            SyscallResult::Ok(0)
        )
    }

    /// End the guest; the hypervisor exits after it.
    pub fn terminate(&self) {
        let _ = sys_post(self.hypervisor_tid, &[VMM_OP_TERMINATE_APP, 0, 0, 0]);
    }

    /// The next event the hypervisor posted, if any.
    pub fn next_event(&self) -> Option<VmmEvent> {
        let mut buf = [0u8; VmmEvent::LEN];
        match sys_try_recv(self.hypervisor_tid, &mut buf) {
            SyscallResult::Ok(sender) if sender == self.hypervisor_tid => VmmEvent::decode(&buf),
            _ => None,
        }
    }
}

/// Input on its way to the guest, in order. The hypervisor drains its mailbox
/// only between guest exits, so a burst (this window catching up after a
/// stall) can fill it; a dropped key release would leave the key held in the
/// guest, so events wait here instead.
pub struct InputOutbox(VecDeque<GuestEvent>);

impl InputOutbox {
    pub const fn new() -> Self {
        Self(VecDeque::new())
    }

    /// Queue `event`; past capacity (the hypervisor stopped draining) the
    /// oldest event is dropped.
    pub fn push(&mut self, event: GuestEvent) {
        if self.0.len() == OUTBOX_CAPACITY {
            self.0.pop_front();
        }
        self.0.push_back(event);
    }

    /// Post queued events in order until the mailbox is full. Returns `true`
    /// while events remain.
    pub fn flush(&mut self, guest: &Guest) -> bool {
        while let Some(&event) = self.0.front() {
            if !guest.forward(event) {
                return true;
            }
            self.0.pop_front();
        }
        false
    }
}
