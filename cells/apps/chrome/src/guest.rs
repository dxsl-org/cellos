//! The Linux guest behind the window: this launcher's own hypervisor Cell and
//! the `vmm_session` protocol to it.

use alloc::string::ToString;

use api::services::vmm_session::{
    VmmEvent, VmmInputForwardRequest, VmmLaunchReply, VmmLaunchRequest, VmmResizeRequest,
    VMM_OP_TERMINATE_APP, VMM_STATUS_OK,
};
use ostd::syscall::{sys_post, sys_recv_timeout, sys_try_recv, SyscallResult};

/// How long the hypervisor may take to answer LAUNCH (10 ms ticks).
const LAUNCH_REPLY_TICKS: u64 = 500;

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

    /// Forward one evdev event; dropped if the hypervisor's queue is full.
    pub fn forward(&self, event_type: u16, code: u16, value: i32) {
        let request = VmmInputForwardRequest::new(self.session_id, event_type, code, value);
        let mut buf = [0u8; VmmInputForwardRequest::LEN];
        if request.encode(&mut buf).is_ok() {
            let _ = sys_post(self.hypervisor_tid, &buf);
        }
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
