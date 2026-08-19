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
    api::caller_identity::CallerIdentity::from_recv_buf(buf).is_some_and(|identity| {
        identity.cell_id == broker_tid as u64
            && identity.generation != 0
            && identity.sender_tid == sender as u64
    })
}
