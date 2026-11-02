//! Fixed 64-byte frames exchanged by the recovery-demo client, worker and
//! authority. Same-build, provider-private records (not kernel ABI): byte 0 is
//! the tag, byte 1 the version (exactly 1), byte 2 a per-tag code, byte 3 is
//! reserved zero, bytes 4..8 a little-endian `u32`, then seven `u64` words.
//! Decoding refuses a short buffer, another version, a non-zero reserved byte
//! and an unknown tag before anything acts on the frame.

pub const FRAME_LEN: usize = 64;
const VERSION: u8 = 1;

/// Request binding committed atomically with its mutation (CP1 §4.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binding {
    pub caller_cell: u64,
    pub caller_generation: u64,
    pub request_id: u64,
    pub method: u8,
    pub rights: u32,
    /// Canonical payload: the operand itself, so no digest is needed.
    pub payload: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    Conflict = 1,
    StaleEpoch = 2,
    RevisionMismatch = 3,
    RecoveryFailed = 4,
    QuotaExceeded = 5,
    Denied = 6,
    InvalidMutation = 7,
}

/// Client-requested crash point inside the worker (fault-inject builds act on it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    None = 0,
    BeforeCommit = 1,
    AfterCommit = 2,
    /// Stage the mutation, wait for `Go`, then commit (delayed old writer).
    Stall = 3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Msg {
    // worker → authority
    Attach {
        nonce: u64,
    },
    Restored {
        epoch: u64,
        revision: u64,
    },
    RestoreFailed {
        epoch: u64,
    },
    Commit {
        epoch: u64,
        expected_revision: u64,
        binding: Binding,
        new_state: u64,
    },
    Lookup {
        epoch: u64,
        binding: Binding,
    },
    // authority → worker
    Attached {
        epoch: u64,
        revision: u64,
        schema: u32,
        counter: u64,
    },
    Committed {
        revision: u64,
        result: u64,
    },
    Present {
        revision: u64,
        result: u64,
    },
    Absent,
    Refused(Refusal),
    // client → authority
    Report,
    Fence,
    Corrupt {
        kind: u8,
    },
    Heal,
    /// Share a fresh authority-owned page read-write with the current worker.
    Lend,
    // authority → client
    Status {
        state: u8,
        worker: u64,
        epoch: u64,
        revision: u64,
        counter: u64,
        restarts: u64,
        ledger: u64,
        lock_word: u64,
    },
    Lent {
        grant: u64,
    },
    // client → worker
    Mutate {
        request_id: u64,
        method: u8,
        amount: u64,
        fault: Fault,
    },
    Get,
    Go,
    Crash {
        mode: u8,
        arg: u64,
    },
    // worker → client
    Done {
        request_id: u64,
        result: u64,
        revision: u64,
        replayed: bool,
    },
    Value {
        counter: u64,
        revision: u64,
        epoch: u64,
    },
    Staged {
        request_id: u64,
    },
    Failed {
        request_id: u64,
        refusal: Refusal,
    },
    Ack,
}

struct Raw {
    tag: u8,
    code: u8,
    half: u32,
    w: [u64; 7],
}
impl Raw {
    fn new(tag: u8) -> Self {
        Self {
            tag,
            code: 0,
            half: 0,
            w: [0; 7],
        }
    }
}

fn refusal(code: u8) -> Option<Refusal> {
    use Refusal::*;
    [
        Conflict,
        StaleEpoch,
        RevisionMismatch,
        RecoveryFailed,
        QuotaExceeded,
        Denied,
        InvalidMutation,
    ]
    .into_iter()
    .find(|r| *r as u8 == code)
}

fn fault(code: u8) -> Option<Fault> {
    [
        Fault::None,
        Fault::BeforeCommit,
        Fault::AfterCommit,
        Fault::Stall,
    ]
    .into_iter()
    .find(|f| *f as u8 == code)
}

fn put_binding(r: &mut Raw, b: &Binding, first: usize) {
    r.code = b.method;
    r.half = b.rights;
    r.w[first..first + 4].copy_from_slice(&[
        b.caller_cell,
        b.caller_generation,
        b.request_id,
        b.payload,
    ]);
}

fn binding(r: &Raw, first: usize) -> Binding {
    Binding {
        caller_cell: r.w[first],
        caller_generation: r.w[first + 1],
        request_id: r.w[first + 2],
        method: r.code,
        rights: r.half,
        payload: r.w[first + 3],
    }
}

impl Msg {
    pub fn encode(&self) -> [u8; FRAME_LEN] {
        let r = match *self {
            Msg::Attach { nonce } => Raw {
                w: [nonce, 0, 0, 0, 0, 0, 0],
                ..Raw::new(0x11)
            },
            Msg::Restored { epoch, revision } => Raw {
                w: [epoch, revision, 0, 0, 0, 0, 0],
                ..Raw::new(0x12)
            },
            Msg::RestoreFailed { epoch } => Raw {
                w: [epoch, 0, 0, 0, 0, 0, 0],
                ..Raw::new(0x13)
            },
            Msg::Commit {
                epoch,
                expected_revision,
                binding,
                new_state,
            } => {
                let mut r = Raw {
                    w: [epoch, expected_revision, 0, 0, 0, 0, new_state],
                    ..Raw::new(0x14)
                };
                put_binding(&mut r, &binding, 2);
                r
            }
            Msg::Lookup { epoch, binding } => {
                let mut r = Raw {
                    w: [epoch, 0, 0, 0, 0, 0, 0],
                    ..Raw::new(0x15)
                };
                put_binding(&mut r, &binding, 2);
                r
            }
            Msg::Attached {
                epoch,
                revision,
                schema,
                counter,
            } => Raw {
                half: schema,
                w: [epoch, revision, counter, 0, 0, 0, 0],
                ..Raw::new(0x21)
            },
            Msg::Committed { revision, result } => Raw {
                w: [revision, result, 0, 0, 0, 0, 0],
                ..Raw::new(0x22)
            },
            Msg::Present { revision, result } => Raw {
                w: [revision, result, 0, 0, 0, 0, 0],
                ..Raw::new(0x23)
            },
            Msg::Absent => Raw::new(0x24),
            Msg::Refused(code) => Raw {
                code: code as u8,
                ..Raw::new(0x25)
            },
            Msg::Report => Raw::new(0x31),
            Msg::Fence => Raw::new(0x32),
            Msg::Corrupt { kind } => Raw {
                code: kind,
                ..Raw::new(0x33)
            },
            Msg::Heal => Raw::new(0x34),
            Msg::Lend => Raw::new(0x35),
            Msg::Lent { grant } => Raw {
                w: [grant, 0, 0, 0, 0, 0, 0],
                ..Raw::new(0x42)
            },
            Msg::Status {
                state,
                worker,
                epoch,
                revision,
                counter,
                restarts,
                ledger,
                lock_word,
            } => Raw {
                code: state,
                w: [
                    worker, epoch, revision, counter, restarts, ledger, lock_word,
                ],
                ..Raw::new(0x41)
            },
            Msg::Mutate {
                request_id,
                method,
                amount,
                fault,
            } => Raw {
                code: method,
                half: fault as u32,
                w: [request_id, amount, 0, 0, 0, 0, 0],
                ..Raw::new(0x51)
            },
            Msg::Get => Raw::new(0x52),
            Msg::Go => Raw::new(0x53),
            Msg::Crash { mode, arg } => Raw {
                code: mode,
                w: [arg, 0, 0, 0, 0, 0, 0],
                ..Raw::new(0x54)
            },
            Msg::Done {
                request_id,
                result,
                revision,
                replayed,
            } => Raw {
                code: replayed as u8,
                w: [request_id, result, revision, 0, 0, 0, 0],
                ..Raw::new(0x61)
            },
            Msg::Value {
                counter,
                revision,
                epoch,
            } => Raw {
                w: [counter, revision, epoch, 0, 0, 0, 0],
                ..Raw::new(0x62)
            },
            Msg::Staged { request_id } => Raw {
                w: [request_id, 0, 0, 0, 0, 0, 0],
                ..Raw::new(0x63)
            },
            Msg::Failed {
                request_id,
                refusal,
            } => Raw {
                code: refusal as u8,
                w: [request_id, 0, 0, 0, 0, 0, 0],
                ..Raw::new(0x64)
            },
            Msg::Ack => Raw::new(0x65),
        };
        let mut out = [0u8; FRAME_LEN];
        out[0] = r.tag;
        out[1] = VERSION;
        out[2] = r.code;
        out[4..8].copy_from_slice(&r.half.to_le_bytes());
        for (i, word) in r.w.iter().enumerate() {
            out[8 + i * 8..16 + i * 8].copy_from_slice(&word.to_le_bytes());
        }
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<Msg> {
        let bytes = bytes.get(..FRAME_LEN)?;
        if bytes[1] != VERSION || bytes[3] != 0 {
            return None;
        }
        let mut r = Raw::new(bytes[0]);
        r.code = bytes[2];
        r.half = u32::from_le_bytes(bytes[4..8].try_into().ok()?);
        for i in 0..7 {
            r.w[i] = u64::from_le_bytes(bytes[8 + i * 8..16 + i * 8].try_into().ok()?);
        }
        let w = r.w;
        Some(match r.tag {
            0x11 => Msg::Attach { nonce: w[0] },
            0x12 => Msg::Restored {
                epoch: w[0],
                revision: w[1],
            },
            0x13 => Msg::RestoreFailed { epoch: w[0] },
            0x14 => Msg::Commit {
                epoch: w[0],
                expected_revision: w[1],
                binding: binding(&r, 2),
                new_state: w[6],
            },
            0x15 => Msg::Lookup {
                epoch: w[0],
                binding: binding(&r, 2),
            },
            0x21 => Msg::Attached {
                epoch: w[0],
                revision: w[1],
                schema: r.half,
                counter: w[2],
            },
            0x22 => Msg::Committed {
                revision: w[0],
                result: w[1],
            },
            0x23 => Msg::Present {
                revision: w[0],
                result: w[1],
            },
            0x24 => Msg::Absent,
            0x25 => Msg::Refused(refusal(r.code)?),
            0x31 => Msg::Report,
            0x32 => Msg::Fence,
            0x33 => Msg::Corrupt { kind: r.code },
            0x34 => Msg::Heal,
            0x35 => Msg::Lend,
            0x42 => Msg::Lent { grant: w[0] },
            0x41 => Msg::Status {
                state: r.code,
                worker: w[0],
                epoch: w[1],
                revision: w[2],
                counter: w[3],
                restarts: w[4],
                ledger: w[5],
                lock_word: w[6],
            },
            0x51 => Msg::Mutate {
                request_id: w[0],
                method: r.code,
                amount: w[1],
                fault: u8::try_from(r.half).ok().and_then(fault)?,
            },
            0x52 => Msg::Get,
            0x53 => Msg::Go,
            0x54 => Msg::Crash {
                mode: r.code,
                arg: w[0],
            },
            0x61 => Msg::Done {
                request_id: w[0],
                result: w[1],
                revision: w[2],
                replayed: r.code != 0,
            },
            0x62 => Msg::Value {
                counter: w[0],
                revision: w[1],
                epoch: w[2],
            },
            0x63 => Msg::Staged { request_id: w[0] },
            0x64 => Msg::Failed {
                request_id: w[0],
                refusal: refusal(r.code)?,
            },
            0x65 => Msg::Ack,
            _ => return None,
        })
    }
}
