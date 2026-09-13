//! Real target transport: typed IPC to the registered AI inference service.
//!
//! Compiled only for bare-metal targets with the `ostd-transport` feature, so host tests never link
//! `ostd`'s global allocator. The transport reuses `ostd`'s sender-masked service call (Spec 17
//! §7): a reply from any cell other than the resolved service is a failure, never decoded.

use ai_proto::{AiRequest, AiResponse, AI_IPC_BUF_SIZE};
use ostd::ipc::{service_call, IpcError};
use ostd::service::{service, ServiceRef};

use crate::transport::AiTransport;
use crate::AiClientError;

/// Transport backed by the live `service::AI` provider.
pub struct OstdTransport {
    svc: ServiceRef<{ service::AI }>,
}

impl OstdTransport {
    /// Create an unresolved transport; resolution happens on the first call and is cached.
    pub const fn new() -> Self {
        Self {
            svc: ServiceRef::new(),
        }
    }

    /// Drop the cached provider tid (for example after the supervisor respawned the service).
    pub fn invalidate(&mut self) {
        self.svc.invalidate();
    }
}

impl Default for OstdTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl AiTransport for OstdTransport {
    fn round_trip<'r>(
        &mut self,
        request: &AiRequest<'_>,
        reply: &'r mut [u8],
    ) -> Result<AiResponse<'r>, AiClientError> {
        let tid = self.svc.resolve().ok_or(AiClientError::NoService)?;
        let mut send = [0u8; AI_IPC_BUF_SIZE];
        match service_call(tid, request, &mut send, reply) {
            // The receive buffer arrives without a length (the syscall reports the sender), so the
            // reply is decoded tolerantly — the same convention every other typed client uses.
            Ok(raw) => ai_proto::decode(raw).map_err(|_| AiClientError::Protocol),
            Err(IpcError::Encode) | Err(IpcError::Decode) => Err(AiClientError::Protocol),
            Err(IpcError::Send) | Err(IpcError::Recv) | Err(IpcError::WrongSender) => {
                // The provider may have died or restarted under a new tid.
                self.svc.invalidate();
                Err(AiClientError::Transport)
            }
        }
    }
}
