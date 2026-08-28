pub fn run() -> ! {
    super::c2c_broker_oracle_orchestrator::run()
}

pub fn run_client() -> ! {
    super::c2c_broker_oracle_client::run_client()
}

pub(super) fn recv_from_broker(broker_tid: usize, buf: &mut [u8]) -> bool {
    let sender = match ostd::syscall::sys_recv_attested(0, buf) {
        ostd::syscall::SyscallResult::Ok(sender) if sender > 0 => sender,
        _ => return false,
    };
    let Some(identity) = api::caller_identity::CallerIdentity::from_recv_buf(buf) else {
        return false;
    };
    identity.generation != 0
        && identity.sender_tid == sender as u64
        && ostd::syscall::sys_resolve_cell_owner(identity.cell_id, identity.generation)
            .is_some_and(|owner| owner.is_live() && owner.root_tid == broker_tid as u64)
}
