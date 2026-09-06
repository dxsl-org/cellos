//! IPC send/recv round-trip latency benchmark.
//!
//! Spawns a private echo peer and measures a 64-byte request followed by the
//! peer's one-byte zero reply. The receive mask and returned sender metadata
//! bind every reply to that peer; untouched sentinel bytes distinguish the
//! one-byte reply without changing the public syscall ABI.

use api::{benchmark::ViBenchmark, ViError};
use ostd::syscall::{
    sys_force_exit, sys_recv, sys_send, sys_set_spawn_args, sys_spawn_pinned, SyscallResult,
};

// Use bench-probe (VA 0x19000000) so the echo peer doesn't collide with the
// orchestrator's pages (VA 0x18000000) in the shared SAS page table.
const SELF_PATH: &str = "/bin/bench-probe";

pub struct IpcSendRecvBench {
    echo_tid: usize,
    msg: [u8; 64],
    buf: [u8; 64],
}

impl IpcSendRecvBench {
    pub fn new() -> Self {
        let mut msg = [0u8; 64];
        msg[0] = 0x42;
        Self {
            echo_tid: 0,
            msg,
            buf: [0u8; 64],
        }
    }
}

impl ViBenchmark for IpcSendRecvBench {
    fn name(&self) -> &'static str {
        "ipc_send_recv"
    }

    fn setup(&mut self) -> api::ViResult<()> {
        if !sys_set_spawn_args("ipc-echo") {
            return Err(ViError::IO);
        }
        self.echo_tid = match sys_spawn_pinned(SELF_PATH, api::TaskPriority::Normal as u8, 0) {
            SyscallResult::Ok(tid) if tid != 0 => tid,
            _ => return Err(ViError::NotFound),
        };
        for _ in 0..20 {
            ostd::task::yield_now();
        }
        Ok(())
    }

    fn run_once(&mut self) -> api::ViResult<u64> {
        if self.echo_tid == 0 {
            return Err(ViError::NotFound);
        }

        self.buf.fill(0xa5);
        if !matches!(sys_send(self.echo_tid, &self.msg), SyscallResult::Ok(_)) {
            return Err(ViError::IO);
        }
        match sys_recv(self.echo_tid, &mut self.buf) {
            SyscallResult::Ok(sender) if sender == self.echo_tid => {}
            SyscallResult::Ok(_) => return Err(ViError::InvalidInput),
            SyscallResult::Err(_) => return Err(ViError::IO),
        }
        if self.buf[0] != 0 || self.buf[1..].iter().any(|&byte| byte != 0xa5) {
            return Err(ViError::InvalidInput);
        }
        Ok(0)
    }

    fn teardown(&mut self) -> api::ViResult<()> {
        if self.echo_tid == 0 {
            return Ok(());
        }
        match sys_force_exit(self.echo_tid) {
            SyscallResult::Ok(_) => {
                self.echo_tid = 0;
                Ok(())
            }
            SyscallResult::Err(_) => Err(ViError::IO),
        }
    }
}

impl Drop for IpcSendRecvBench {
    fn drop(&mut self) {
        if self.echo_tid != 0 {
            let _ = sys_force_exit(self.echo_tid);
        }
    }
}

impl Default for IpcSendRecvBench {
    fn default() -> Self {
        Self::new()
    }
}
