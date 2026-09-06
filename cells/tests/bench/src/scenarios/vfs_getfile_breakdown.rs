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
//! The breakdown intentionally avoids service discovery and filesystem work.
//! The echo peer performs the same typed exchange (decode a request, encode a
//! `DataPtr` reply) but no lookup. That makes the measured round trip the
//! **transport cost alone** — the numerator of the fraction above — and leaves
//! the handler body to be added to the denominator separately.
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

use api::{
    benchmark::ViBenchmark,
    ipc::{VfsRequest, VfsResponse, IPC_BUF_SIZE},
    ViError,
};
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
pub fn run_resp_echo() -> ! {
    let mut rx = [0u8; IPC_BUF_SIZE];
    let mut tx = [0u8; 64];
    loop {
        rx.fill(0);
        let sender = match syscall::sys_recv(0, &mut rx) {
            SyscallResult::Ok(sid) if sid != 0 => sid,
            _ => continue,
        };
        let request: Result<VfsRequest, _> = api::ipc::decode(&rx);
        let encoded = request
            .and_then(|_| api::ipc::encode(&REPLY, &mut tx))
            .map(|bytes| &bytes[..]);
        match encoded {
            Ok(bytes) => {
                let _ = syscall::sys_send(sender, bytes);
            }
            Err(_) => {
                let _ = syscall::sys_send(sender, &[0xff]);
            }
        }
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
            let encoded =
                api::ipc::encode(&req, &mut self.buf).map_err(|_| ViError::InvalidInput)?;
            let _ = black_box(encoded);
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
        Self {
            encoded: [0u8; 64],
            len: 0,
        }
    }
}

impl ViBenchmark for DecodeReplyBench {
    fn name(&self) -> &'static str {
        "stage_decode_reply_x1000"
    }

    fn setup(&mut self) -> api::ViResult<()> {
        self.len = api::ipc::encode(&REPLY, &mut self.encoded)
            .map_err(|_| ViError::InvalidInput)?
            .len();
        Ok(())
    }

    fn run_once(&mut self) -> api::ViResult<u64> {
        let bytes = &self.encoded[..self.len];
        for _ in 0..INNER {
            let response: VfsResponse =
                api::ipc::decode(black_box(bytes)).map_err(|_| ViError::InvalidInput)?;
            let _ = black_box(response);
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
        Self {
            peer: 0,
            send_buf: [0u8; 512],
            recv_buf: [0u8; IPC_BUF_SIZE],
        }
    }
}

impl ViBenchmark for RoundTripBench {
    fn name(&self) -> &'static str {
        "total_typed_roundtrip_x1000"
    }

    fn setup(&mut self) -> api::ViResult<()> {
        if !syscall::sys_set_spawn_args("resp-echo") {
            return Err(ViError::IO);
        }
        self.peer = match syscall::sys_spawn_pinned(PROBE_PATH, api::TaskPriority::Normal as u8, 0)
        {
            SyscallResult::Ok(tid) if tid != 0 => tid,
            _ => return Err(ViError::NotFound),
        };
        for _ in 0..20 {
            ostd::task::yield_now();
        }
        Ok(())
    }

    fn run_once(&mut self) -> api::ViResult<u64> {
        if self.peer == 0 {
            return Err(ViError::NotFound);
        }
        for _ in 0..INNER {
            let req = VfsRequest::GetFile(black_box(PATH));
            let n = api::ipc::encode(&req, &mut self.send_buf)
                .map_err(|_| ViError::InvalidInput)?
                .len();
            if !matches!(
                syscall::sys_send(self.peer, &self.send_buf[..n]),
                SyscallResult::Ok(_)
            ) {
                return Err(ViError::IO);
            }
            self.recv_buf.fill(0);
            match syscall::sys_recv(self.peer, &mut self.recv_buf) {
                SyscallResult::Ok(sender) if sender == self.peer => {}
                SyscallResult::Ok(_) => return Err(ViError::InvalidInput),
                SyscallResult::Err(_) => return Err(ViError::IO),
            }
            let response: VfsResponse =
                api::ipc::decode(&self.recv_buf).map_err(|_| ViError::InvalidInput)?;
            let _ = black_box(response);
        }
        Ok(0)
    }

    fn teardown(&mut self) -> api::ViResult<()> {
        if self.peer == 0 {
            return Ok(());
        }
        match syscall::sys_force_exit(self.peer) {
            SyscallResult::Ok(_) => {
                self.peer = 0;
                Ok(())
            }
            SyscallResult::Err(_) => Err(ViError::IO),
        }
    }
}

impl Drop for RoundTripBench {
    fn drop(&mut self) {
        if self.peer != 0 {
            let _ = syscall::sys_force_exit(self.peer);
        }
    }
}
