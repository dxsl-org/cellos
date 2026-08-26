//! Attested KMS-only ingress for the development Silo provider.

use api::caller_identity::CallerIdentity;
use api::syscall::service;
use ostd::syscall::{sys_lookup_service, sys_recv_attested, sys_try_send, SyscallResult};
use service_silo::protocol::{PeerIdentity, ProtocolState};
use types::silo::DEVELOPMENT_SILO_FRAME_LEN;

use crate::guest::GuestSession;

/// Serve requests forever; only a kernel-attested live KMS instance can bind.
pub fn run(mut guest: GuestSession) -> ! {
    let mut state = ProtocolState::new();
    loop {
        let mut buffer = [0u8; api::ipc::IPC_BUF_SIZE];
        let sender = match sys_recv_attested(0, &mut buffer) {
            SyscallResult::Ok(sender) if sender > 0 => sender,
            _ => continue,
        };
        let peer = CallerIdentity::from_recv_buf(&buffer).map(|identity| PeerIdentity {
            sender_tid: identity.sender_tid as usize,
            cell_id: identity.cell_id,
            generation: identity.generation,
        });
        let response = state.process(
            &mut guest,
            sender,
            sys_lookup_service(service::KMS),
            peer,
            &buffer[..DEVELOPMENT_SILO_FRAME_LEN],
        );
        if let Some(response) = response {
            let _ = sys_try_send(sender, &response.encode());
        }
    }
}
