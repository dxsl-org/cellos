//! Stage breakdown of a typed VFS-shaped request/reply on the message path.
//!
//! Answers one question: of the cost of a `GetFile`, how much is spent on work a
//! by-reference direct call would skip? A direct call skips the whole IPC
//! rendezvous — encoding the request, both `ecall`s, and both context switches —
//! and runs the handler on the caller's own thread. It does **not** skip the
//! handler body, nor encoding and decoding the reply (the fast handler still
//! ends in `api::ipc::encode`), nor resolving the caller's identity.
//!
//! So: `saving ≈ round_trip`, `total ≈ round_trip + handler_body`, and the
//! fraction saved is `round_trip / (round_trip + handler_body)`.
//!
//! # Why the peer is an echo and not the real VFS
//! The bench cell's syscall allowlist grants neither `LookupService` nor
//! `RecvTimeout`, so it cannot discover the VFS tid, and widening the allowlist
//! would change the system being measured. The echo peer therefore stands in for
//! the service: it performs the same typed exchange (decode a request, encode a
//! `DataPtr` reply) but no filesystem lookup. That makes the measured round trip
//! the **transport cost alone** — the numerator of the fraction above — and
//! leaves the handler body to be added to the denominator separately.
//!
//! # Why each scenario loops internally
//! The shared runner brackets every `run_once` with two `sys_get_time` calls,
//! and `sys_get_time` is itself a syscall — the same order of magnitude as the
//! stages being measured. Each scenario performs `INNER` operations per
//! `run_once`, so the bracket amortises to noise. Divide a reported figure by
//! `INNER` for the per-operation cost.
//!
//! # Reading the numbers
//! QEMU TCG does not model a pipeline, so absolute figures are indicative only
//! and traps are cheaper relative to compute than on hardware. The *ratio*
//! between stages is what this measurement is for, and a ratio that favours a
//! direct call here must still be confirmed on a board.

use api::benchmark::ViBenchmark;
use api::ipc::{VfsRequest, VfsResponse, IPC_BUF_SIZE};
use core::hint::black_box;
use ostd::syscall::{self, sys_get_time, SyscallResult};

/// Operations per `run_once`, chosen so the runner's two-syscall bracket is
/// under a per-mille of a sample.
const INNER: u32 = 1_000;

/// Representative absolute path: long enough that postcard writes a realistic
/// length-prefixed string, short enough to be an ordinary lookup.
const PATH: &str = "/bin/hello-cell";

/// The reply shape a real `GetFile` returns on both paths.
const REPLY: VfsResponse = VfsResponse::DataPtr {
    ptr: 0x8000_0000,
    len: 4096,
};

/// Echo peer binary — a second instance of this cell in the `resp-echo` role.
/// Separate VA base (0x19000000) so it does not collide with the orchestrator.
const PROBE_PATH: &str = "/bin/bench-probe";

/// Peer loop: decode a typed request, reply with an encoded `DataPtr`.
///
/// Mirrors what a service does minus the lookup, so the measured round trip is
/// transport cost with no filesystem work folded in.
pub fn run_resp_echo() -> ! {
    let mut rx = [0u8; IPC_BUF_SIZE];
    let mut tx = [0u8; 64];
    let n = api::ipc::encode(&REPLY, &mut tx).map(|s| s.len()).unwrap_or(0);
    loop {
        let sender = match syscall::sys_recv(0, &mut rx) {
            SyscallResult::Ok(sid) => sid,
            _ => continue,
        };
        // Decode so the peer pays the same deserialisation a service would.
        let _: Result<VfsRequest, _> = api::ipc::decode(&rx);
        syscall::sys_send(sender, &tx[..n]);
    }
}

/// Stage 1 — encode the request. Marshalling a direct call avoids entirely.
pub struct EncodeRequestBench {
    buf: [u8; 512],
}

impl EncodeRequestBench {
    pub fn new() -> Self {
        Self { buf: [0u8; 512] }
    }
}

impl ViBenchmark for EncodeRequestBench {
    fn name(&self) -> &'static str {
        "stage_encode_request_x1000"
    }

    fn run_once(&mut self) -> api::ViResult<u64> {
        for _ in 0..INNER {
            let req = VfsRequest::GetFile(black_box(PATH));
            let _ = black_box(api::ipc::encode(&req, &mut self.buf));
        }
        Ok(0)
    }
}

/// Stage 2 — decode the reply. Both paths pay this; measured to prove it is
/// *not* part of the saving.
pub struct DecodeReplyBench {
    encoded: [u8; 64],
    len: usize,
}

impl DecodeReplyBench {
    pub fn new() -> Self {
        let mut encoded = [0u8; 64];
        let len = api::ipc::encode(&REPLY, &mut encoded)
            .map(|s| s.len())
            .unwrap_or(0);
        Self { encoded, len }
    }
}

impl ViBenchmark for DecodeReplyBench {
    fn name(&self) -> &'static str {
        "stage_decode_reply_x1000"
    }

    fn run_once(&mut self) -> api::ViResult<u64> {
        let bytes = &self.encoded[..self.len];
        for _ in 0..INNER {
            let r: Result<VfsResponse, _> = api::ipc::decode(black_box(bytes));
            let _ = black_box(r);
        }
        Ok(0)
    }
}

/// Stage 3 — a bare `ecall` round trip.
///
/// `sys_get_time` reads a counter and returns, so its cost is dominated by trap
/// entry and exit rather than by the work performed.
pub struct TrapRoundTripBench;

impl ViBenchmark for TrapRoundTripBench {
    fn name(&self) -> &'static str {
        "stage_ecall_roundtrip_x1000"
    }

    fn run_once(&mut self) -> api::ViResult<u64> {
        for _ in 0..INNER {
            let _ = black_box(sys_get_time());
        }
        Ok(0)
    }
}

/// Total — a full typed request/reply rendezvous: encode, send, switch to the
/// peer, peer decode + encode, switch back, recv, decode.
///
/// This is the numerator of the saving: everything here is skipped when the
/// caller runs the handler itself.
pub struct RoundTripBench {
    peer: usize,
    send_buf: [u8; 512],
    recv_buf: [u8; IPC_BUF_SIZE],
}

impl RoundTripBench {
    pub fn new() -> Self {
        syscall::sys_set_spawn_args("resp-echo");
        let peer = match syscall::sys_spawn_pinned(PROBE_PATH, api::TaskPriority::Normal as u8, 0) {
            SyscallResult::Ok(tid) => tid,
            _ => 0,
        };
        // Let the peer reach its recv loop before the first send: `sys_send`
        // only lands on a task already parked in Recv.
        for _ in 0..20 {
            ostd::task::yield_now();
        }
        Self {
            peer,
            send_buf: [0u8; 512],
            recv_buf: [0u8; IPC_BUF_SIZE],
        }
    }
}

impl ViBenchmark for RoundTripBench {
    fn name(&self) -> &'static str {
        "total_typed_roundtrip_x1000"
    }

    fn run_once(&mut self) -> api::ViResult<u64> {
        if self.peer == 0 {
            return Ok(0);
        }
        for _ in 0..INNER {
            let req = VfsRequest::GetFile(black_box(PATH));
            let n = match api::ipc::encode(&req, &mut self.send_buf) {
                Ok(s) => s.len(),
                Err(_) => return Ok(0),
            };
            syscall::sys_send(self.peer, &self.send_buf[..n]);
            // Masked recv against the peer, per the wire contract's recv-mask
            // rule: a wildcard recv could consume another sender's message and
            // desync the exchange.
            if let SyscallResult::Ok(_) = syscall::sys_recv(self.peer, &mut self.recv_buf) {
                let r: Result<VfsResponse, _> = api::ipc::decode(&self.recv_buf);
                let _ = black_box(r);
            }
        }
        Ok(0)
    }
}
