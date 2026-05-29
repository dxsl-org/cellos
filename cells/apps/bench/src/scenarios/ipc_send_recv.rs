//! IPC send/recv round-trip latency benchmark.
//!
//! Sends a 64-byte message to the VFS Cell (always listening) and waits for
//! any reply.  This is a proxy for generic IPC overhead; the VFS Cell returns
//! an empty reply to unknown opcodes.  PDR target: < 50 µs per round-trip.

use api::benchmark::ViBenchmark;
use ostd::syscall::{sys_send, sys_recv};

/// Well-known VFS Cell endpoint ID (matches cells/services/vfs main loop).
const VFS_ENDPOINT: usize = 2;

pub struct IpcSendRecvBench {
    msg: [u8; 64],
    buf: [u8; 64],
}

impl IpcSendRecvBench {
    pub fn new() -> Self {
        // Opcode 0xFF → unknown, VFS replies with empty; valid for timing only.
        let mut msg = [0u8; 64];
        msg[0] = 0xFF;
        Self { msg, buf: [0u8; 64] }
    }
}

impl ViBenchmark for IpcSendRecvBench {
    fn name(&self) -> &'static str { "ipc_send_recv" }

    fn run_once(&mut self) -> api::ViResult<u64> {
        sys_send(VFS_ENDPOINT, &self.msg);
        // Receive any reply to drain the queue.
        let _ = sys_recv(0, &mut self.buf);
        Ok(0)
    }
}

impl Default for IpcSendRecvBench {
    fn default() -> Self { Self::new() }
}
