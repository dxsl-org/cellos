//! The launcher session of a hypervisor started as `hypervisor app`.
//!
//! A Tier 3 app launcher (`cells/apps/chrome`) spawns its own hypervisor with
//! the argument `app` and sends `VMM_OP_LAUNCH_APP` with its window's content
//! size. The session is bound to that sender for the VM's whole life: only it
//! may forward input, resize the guest display, or end the guest. Every other
//! sender's message is dropped. The session protocol is
//! `api::services::vmm_session`.

use api::services::vmm_session::{
    self as vmm_ipc, display_size_valid, VmmInputForwardRequest, VmmLaunchReply,
    VmmLaunchRequest, VmmResizeRequest,
};
use ostd::io::println;
use ostd::syscall::{sys_post, sys_recv_timeout, sys_try_recv, SyscallResult};

use crate::virtio_gpu::GpuDev;
use crate::virtio_input::InputDev;
use crate::virtio_mmio::VirtioMmio;

/// One session per VM, so its id is fixed.
const SESSION_ID: u32 = 1;
/// Messages handled per poll, so a flooding launcher cannot starve the vCPU.
const MESSAGES_PER_POLL: usize = 32;
/// How long a spawned hypervisor waits for its launcher's LAUNCH (10 ms ticks).
const LAUNCH_WAIT_TICKS: usize = 1000;

/// Whether this hypervisor was spawned by an app launcher.
pub fn app_mode() -> bool {
    ostd::args().iter().any(|arg| arg == "app")
}

pub struct AppSession {
    client_tid: usize,
}

/// What the launcher asked for in one poll.
pub enum SessionRequest {
    Continue,
    Terminate,
}

/// Wait for the launcher's LAUNCH, answer it, and bind the session.
/// Returns the session and the display size the guest boots with.
pub fn wait_for_launch() -> Option<(AppSession, u32, u32)> {
    let mut buf = [0u8; 64];
    for _ in 0..LAUNCH_WAIT_TICKS / 10 {
        buf.fill(0);
        let SyscallResult::Ok(sender) = sys_recv_timeout(0, &mut buf, 10) else {
            continue;
        };
        if sender as isize <= 0 {
            continue;
        }
        let Ok(request) = VmmLaunchRequest::decode(&buf) else {
            continue;
        };
        let accepted = display_size_valid(request.width, request.height);
        let reply = VmmLaunchReply {
            status: if accepted {
                vmm_ipc::VMM_STATUS_OK
            } else {
                vmm_ipc::VMM_STATUS_INVALID_ARG
            },
            session_id: SESSION_ID,
            reserved: 0,
            scanout_width: request.width,
            scanout_height: request.height,
        };
        let mut reply_buf = [0u8; VmmLaunchReply::LEN];
        if reply.encode(&mut reply_buf).is_ok() {
            let _ = sys_post(sender, &reply_buf);
        }
        if !accepted {
            println(&alloc::format!(
                "[hv-x86] launch refused: display {}x{} out of range",
                request.width,
                request.height
            ));
            continue;
        }
        println(&alloc::format!(
            "[hv-x86] app session bound to tid={} ({}x{})",
            sender,
            request.width,
            request.height
        ));
        return Some((AppSession { client_tid: sender }, request.width, request.height));
    }
    println("[hv-x86] no launcher LAUNCH arrived; exiting");
    None
}

impl AppSession {
    pub fn client_tid(&self) -> usize {
        self.client_tid
    }

    /// Tell the launcher the guest will not run (boot failed before the run
    /// loop, which otherwise reports the exit through the GPU presenter).
    pub fn report_exit(&self, code: u32) {
        let event = api::services::vmm_session::VmmEvent::Exited { code };
        let _ = sys_post(self.client_tid, &event.encode());
    }

    /// Drain the launcher's queued requests into the devices.
    pub fn poll(
        &mut self,
        gpu: &mut GpuDev,
        gpu_vmio: &mut VirtioMmio,
        input: &mut InputDev,
    ) -> SessionRequest {
        let mut buf = [0u8; 64];
        for _ in 0..MESSAGES_PER_POLL {
            buf.fill(0);
            let SyscallResult::Ok(sender) = sys_try_recv(0, &mut buf) else {
                break;
            };
            if sender as isize <= 0 {
                break;
            }
            if sender != self.client_tid {
                continue;
            }
            match buf[0] {
                vmm_ipc::VMM_OP_FORWARD_INPUT => {
                    if let Ok(request) = VmmInputForwardRequest::decode(&buf) {
                        if request.session_id == SESSION_ID {
                            forward(input, &request);
                        }
                    }
                }
                vmm_ipc::VMM_OP_RESIZE_APP => {
                    let Ok(request) = VmmResizeRequest::decode(&buf) else {
                        continue;
                    };
                    if request.session_id == SESSION_ID
                        && display_size_valid(request.width, request.height)
                        && gpu.resize(request.width, request.height)
                    {
                        gpu_vmio.signal_config_change();
                    }
                }
                vmm_ipc::VMM_OP_TERMINATE_APP => {
                    println("[hv-x86] app session terminated by its launcher");
                    return SessionRequest::Terminate;
                }
                _ => {}
            }
        }
        SessionRequest::Continue
    }
}

fn forward(input: &mut InputDev, request: &VmmInputForwardRequest) {
    match (request.event_type, request.code) {
        (vmm_ipc::EV_KEY, code) => input.push_key(code, request.value != 0),
        (vmm_ipc::EV_ABS, axis @ (vmm_ipc::ABS_X | vmm_ipc::ABS_Y)) => {
            input.push_abs(axis, request.value.clamp(0, vmm_ipc::ABS_MAX))
        }
        (vmm_ipc::EV_REL, vmm_ipc::REL_WHEEL) => input.push_mouse_scroll(request.value),
        _ => {}
    }
}
