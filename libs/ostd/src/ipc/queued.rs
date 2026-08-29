//! Queued service admission with a bounded, sender-masked reply wait.

use super::IpcError;
use crate::syscall::{sys_recv_timeout, sys_send, SyscallResult};
use serde::{Deserialize, Serialize};

/// Queue one request with blocking `Send`, then bound only the reply wait.
///
/// This transport is for permanent services that may temporarily receive from a
/// nested dependency and therefore cannot admit `TrySend`. A gone target fails
/// the send; a service that accepts but does not reply before `timeout_ticks`
/// returns [`IpcError::Recv`]. After that receive error, callers must poison the
/// service generation because an uncorrelated late reply can still arrive.
pub fn service_call_queued_bounded<'r, Req: Serialize>(
    service_tid: usize,
    req: &Req,
    send_buf: &mut [u8],
    recv_buf: &'r mut [u8],
    timeout_ticks: u64,
) -> Result<&'r [u8], IpcError> {
    let encoded = api::ipc::encode(req, send_buf).map_err(|_| IpcError::Encode)?;
    if let SyscallResult::Err(_) = sys_send(service_tid, encoded) {
        return Err(IpcError::Send);
    }
    match sys_recv_timeout(service_tid, recv_buf, timeout_ticks) {
        SyscallResult::Ok(0) => Err(IpcError::Recv),
        SyscallResult::Ok(sender) if sender == service_tid => Ok(recv_buf),
        SyscallResult::Ok(_) => Err(IpcError::WrongSender),
        SyscallResult::Err(_) => Err(IpcError::Recv),
    }
}

/// Decode the reply from [`service_call_queued_bounded`] as `Resp`.
///
/// `Resp` may borrow `recv_buf`; consume it before reusing that buffer. Errors
/// preserve encode, send, receive-timeout, sender, and decode distinctions.
pub fn service_call_typed_queued_bounded<'r, Req, Resp>(
    service_tid: usize,
    req: &Req,
    send_buf: &mut [u8],
    recv_buf: &'r mut [u8],
    timeout_ticks: u64,
) -> Result<Resp, IpcError>
where
    Req: Serialize,
    Resp: Deserialize<'r>,
{
    let raw = service_call_queued_bounded(service_tid, req, send_buf, recv_buf, timeout_ticks)?;
    api::ipc::decode::<Resp>(raw).map_err(|_| IpcError::Decode)
}
