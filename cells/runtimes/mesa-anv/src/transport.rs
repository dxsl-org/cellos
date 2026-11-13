//! How the shim reaches the Intel GPU driver Cell.

use intel_gpu_core::uapi::ioctl::{EAGAIN, EINVAL, EIO, ENODEV};
use ostd::ipc::{Completion, IpcError, IpcSubmitError, IpcTerminal, PendingCall};

/// Scheduler ticks (10 ms) per wait round.
const WAIT_ROUND_TICKS: u64 = 100;

/// One request/reply exchange with the `XE_GPU` service.
pub trait Transport {
    /// Send `request` and wait for the reply; returns the reply length. Errors are errnos.
    fn call(&mut self, request: &[u8], reply: &mut [u8]) -> Result<usize, i32>;
}

/// `XE_GPU` over kernel-bounded IPC operations (`IpcSubmit` + `IpcTake`, CP1 D-G6): the
/// driver never blocks on this client, and its death ends the wait with `ENODEV`.
pub struct IpcTransport {
    provider: usize,
}

impl IpcTransport {
    /// The live `XE_GPU` provider (`service::XE_GPU`, registered by init per CP1 D-G8).
    pub fn connect() -> Result<Self, i32> {
        ostd::service::provider_tid(abi::syscall::service::XE_GPU)
            .map(|provider| Self { provider })
            .ok_or(ENODEV)
    }
}

impl Transport for IpcTransport {
    fn call(&mut self, request: &[u8], reply: &mut [u8]) -> Result<usize, i32> {
        let call = PendingCall::submit(self.provider, request).map_err(|e| match e {
            IpcSubmitError::Busy => EAGAIN,
            IpcSubmitError::PeerGone => ENODEV,
            IpcSubmitError::InvalidRequest => EINVAL,
        })?;
        // No caller deadline: the kernel ends every operation with a terminal (its own 30 s
        // deadline at the latest), so re-park until it arrives.
        loop {
            match call.wait_and_take(reply, WAIT_ROUND_TICKS, 1) {
                Ok(None) => {}
                Ok(Some(Completion {
                    terminal: IpcTerminal::Reply,
                    len,
                })) => return Ok(len),
                Ok(Some(Completion {
                    terminal: IpcTerminal::PeerGone,
                    ..
                }))
                | Err(IpcError::PeerGone) => return Err(ENODEV),
                Ok(Some(_)) | Err(_) => return Err(EIO),
            }
        }
    }
}
