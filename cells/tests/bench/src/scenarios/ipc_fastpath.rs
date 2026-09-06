// SPDX-License-Identifier: MPL-2.0
//! Zero-Trap Fastpath IPC round-trip latency and throughput benchmark.
//!
//! Evaluates the SPSC Lock-Free Ring Channel over shared memory between
//! two Tier 1 cells in Cellos Single Address Space.
//!
//! Eliminates all ecall traps, context switches, and kernel allocator overhead
//! on the IPC critical path.

#![forbid(unsafe_code)]

use api::{benchmark::ViBenchmark, ViError};
use ostd::ring_channel::ChannelHost;
use ostd::syscall::{
    sys_force_exit, sys_recv, sys_send, sys_set_spawn_args, sys_spawn_pinned, SyscallResult,
};

const SELF_PATH: &str = "/bin/bench-probe";

pub struct IpcFastpathBench {
    echo_tid: usize,
    host: Option<ChannelHost>,
    msg: [u8; 64],
    buf: [u8; 64],
}

impl IpcFastpathBench {
    pub fn new() -> Self {
        let mut msg = [0u8; 64];
        msg[0] = 0x42;
        Self {
            echo_tid: 0,
            host: None,
            msg,
            buf: [0u8; 64],
        }
    }
}

impl ViBenchmark for IpcFastpathBench {
    fn name(&self) -> &'static str {
        "ipc_fastpath"
    }

    fn setup(&mut self) -> api::ViResult<()> {
        let host = ChannelHost::new();
        let handle = host.handle();

        if !sys_set_spawn_args("fastpath-echo") {
            return Err(ViError::IO);
        }

        self.echo_tid = match sys_spawn_pinned(SELF_PATH, api::TaskPriority::Normal as u8, 0) {
            SyscallResult::Ok(tid) if tid != 0 => tid,
            _ => return Err(ViError::NotFound),
        };

        for _ in 0..20 {
            ostd::task::yield_now();
        }

        // Handshake: exchange channel address token via 1 initial syscall
        let handle_bytes = handle.to_le_bytes();
        if !matches!(sys_send(self.echo_tid, &handle_bytes), SyscallResult::Ok(_)) {
            return Err(ViError::IO);
        }

        let mut ack = [0u8; 4];
        match sys_recv(self.echo_tid, &mut ack) {
            SyscallResult::Ok(sender) if sender == self.echo_tid && ack[0] == 0x55 => {}
            _ => return Err(ViError::IO),
        }

        self.host = Some(host);
        Ok(())
    }

    fn run_once(&mut self) -> api::ViResult<u64> {
        let host = self.host.as_ref().ok_or(ViError::NotFound)?;
        let endpoint = host.endpoint();

        self.buf.fill(0xa5);
        let resp_len = endpoint
            .call(&self.msg, &mut self.buf)
            .map_err(|_| ViError::IO)?;

        if resp_len != 1 || self.buf[0] != 0 {
            return Err(ViError::InvalidInput);
        }

        Ok(0)
    }

    fn teardown(&mut self) -> api::ViResult<()> {
        self.host = None;
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

impl Drop for IpcFastpathBench {
    fn drop(&mut self) {
        let _ = self.teardown();
    }
}

impl Default for IpcFastpathBench {
    fn default() -> Self {
        Self::new()
    }
}
