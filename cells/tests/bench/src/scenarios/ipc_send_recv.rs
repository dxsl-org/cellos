//! IPC send/recv round-trip latency benchmark.
//!
//! Spawns a private echo peer and measures a 64-byte request followed by the
//! peer's one-byte zero reply. The receive mask and returned sender metadata
//! bind every reply to that peer; untouched sentinel bytes distinguish the
//! one-byte reply without changing the public syscall ABI.
//!
//! `IpcSendRecvBench::new` uses a Tier-1 SAS peer (`bench-probe ipc-echo`);
//! `IpcSendRecvBench::domain` uses the Tier-2 `bench-peer-t2` peer. A domain
//! peer that cannot be spawned or admitted fails setup; there is no SAS fallback.

use api::{benchmark::ViBenchmark, ViError};
use ostd::syscall::{
    sys_force_exit, sys_recv, sys_send, sys_set_spawn_args, sys_spawn_pinned, SyscallResult,
};

// Use bench-probe (VA 0x19000000) so the echo peer doesn't collide with the
// orchestrator's pages (VA 0x18000000) in the shared SAS page table.
const SAS_PEER_PATH: &str = "/bin/bench-probe";
/// Tier-2 peer: same echo loop, `PROTECTION_CLASS_UNTRUSTED` manifest. Only the
/// x86 lifecycle image packs it, so only the x86 profile runs this scenario.
#[cfg(target_arch = "x86_64")]
const DOMAIN_PEER_PATH: &str = "/bin/bench-peer-t2";

pub struct IpcSendRecvBench {
    name: &'static str,
    peer_path: &'static str,
    echo_tid: usize,
    msg: [u8; 64],
    buf: [u8; 64],
}

impl IpcSendRecvBench {
    pub fn new() -> Self {
        Self::with_peer("ipc_send_recv", SAS_PEER_PATH)
    }

    /// The same round trip against a Tier-2 private-root domain peer.
    #[cfg(target_arch = "x86_64")]
    pub fn domain() -> Self {
        Self::with_peer("ipc_send_recv_domain", DOMAIN_PEER_PATH)
    }

    fn with_peer(name: &'static str, peer_path: &'static str) -> Self {
        let mut msg = [0u8; 64];
        msg[0] = 0x42;
        Self {
            name,
            peer_path,
            echo_tid: 0,
            msg,
            buf: [0u8; 64],
        }
    }
}

impl ViBenchmark for IpcSendRecvBench {
    fn name(&self) -> &'static str {
        self.name
    }

    fn setup(&mut self) -> api::ViResult<()> {
        // The domain peer ignores its arguments; setting them keeps both spawns identical.
        if !sys_set_spawn_args("ipc-echo") {
            return Err(ViError::IO);
        }
        self.echo_tid = match sys_spawn_pinned(self.peer_path, api::TaskPriority::Normal as u8, 0) {
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
