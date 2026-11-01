//! The demo provider's state authority (CP1 option B): the committed counter,
//! its revision, the writer epoch and the request-binding ledger, all in the
//! authority Cell's RAM. It survives a worker crash and nothing else: losing the
//! authority loses the state, which is RecoveryFailed, never a cold start.
//!
//! Contract:
//! - Only the bound writer `(epoch, tid, cell, generation)` may look up or commit.
//! - A commit is atomic with its binding: state, revision and ledger entry change
//!   together or not at all, and only for the expected revision.
//! - The ledger key is `(caller cell, caller generation, request id)`. The same
//!   key with a different method, rights or payload is `Conflict`, never a replay.
//! - `fence` ends the current writer and captures the final committed revision;
//!   a replacement attaches only if the stored state is intact and still at
//!   that watermark.

use alloc::vec::Vec;

use crate::wire::{Binding, Msg, Refusal};

pub const SCHEMA: u32 = 1;
pub const METHOD_INC: u8 = 1;
pub const METHOD_ADD_TWICE: u8 = 2;
const MAX_PER_PRINCIPAL: usize = 64;
const MAX_ENTRIES: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Committed {
    pub schema: u32,
    pub revision: u64,
    pub counter: u64,
    pub checksum: u64,
}

impl Committed {
    pub fn new(schema: u32, revision: u64, counter: u64) -> Self {
        Self { schema, revision, counter, checksum: checksum(schema, revision, counter) }
    }

    /// Detects torn or injected corruption (not tampering: the key is public).
    pub fn intact(&self) -> bool {
        self.checksum == checksum(self.schema, self.revision, self.counter)
    }
}

fn checksum(schema: u32, revision: u64, counter: u64) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    let bytes = schema.to_le_bytes().into_iter().chain(revision.to_le_bytes()).chain(counter.to_le_bytes());
    for byte in bytes {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// The business rule the authority validates every proposed state against.
pub fn apply(method: u8, counter: u64, amount: u64) -> Option<u64> {
    match method {
        METHOD_INC => counter.checked_add(amount),
        METHOD_ADD_TWICE => counter.checked_add(amount.checked_mul(2)?),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Writer {
    None,
    Pending { nonce: u64, tid: usize },
    Bound { epoch: u64, tid: usize, cell: u64, generation: u64 },
    Fenced,
}

#[derive(Clone, Copy)]
struct Entry {
    binding: Binding,
    result: u64,
    revision: u64,
}

pub struct Authority {
    committed: Option<Committed>,
    watermark: u64,
    epoch: u64,
    writer: Writer,
    ledger: Vec<Entry>,
}

/// The kernel-attested sender of a worker frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sender {
    pub tid: usize,
    pub cell: u64,
    pub generation: u64,
}

impl Authority {
    pub fn new() -> Self {
        Self { committed: Some(Committed::new(SCHEMA, 0, 0)), watermark: 0, epoch: 0, writer: Writer::None, ledger: Vec::new() }
    }

    pub fn committed(&self) -> Option<Committed> { self.committed }
    pub fn epoch(&self) -> u64 { self.epoch }
    pub fn writer(&self) -> Writer { self.writer }
    pub fn ledger_len(&self) -> usize { self.ledger.len() }

    /// Expect exactly one child to attach with `nonce` from `tid`.
    pub fn expect_worker(&mut self, nonce: u64, tid: usize) {
        self.writer = Writer::Pending { nonce, tid };
    }

    /// End the current writer and return the final committed watermark. A second
    /// fence keeps the first watermark: nothing committed in between.
    pub fn fence(&mut self) -> u64 {
        if self.writer != Writer::Fenced {
            self.writer = Writer::Fenced;
            self.watermark = self.committed.map_or(self.watermark, |c| c.revision);
        }
        self.watermark
    }

    /// Bind a fresh writer epoch to the attesting child, or refuse.
    pub fn attach(&mut self, nonce: u64, from: Sender) -> Result<(u64, Committed), Refusal> {
        match self.writer {
            Writer::Pending { nonce: expected, tid } if expected == nonce && tid == from.tid => {}
            _ => return Err(Refusal::Denied),
        }
        let state = self.committed.filter(|c| c.intact() && c.revision == self.watermark);
        let Some(state) = state else {
            self.writer = Writer::Fenced;
            return Err(Refusal::RecoveryFailed);
        };
        self.epoch += 1;
        self.writer = Writer::Bound { epoch: self.epoch, tid: from.tid, cell: from.cell, generation: from.generation };
        Ok((self.epoch, state))
    }

    pub fn is_writer(&self, epoch: u64, from: Sender) -> bool {
        self.writer == Writer::Bound { epoch, tid: from.tid, cell: from.cell, generation: from.generation }
    }

    pub fn lookup(&self, epoch: u64, from: Sender, binding: &Binding) -> Msg {
        if !self.is_writer(epoch, from) {
            return Msg::Refused(Refusal::StaleEpoch);
        }
        match self.find(binding) {
            Some(entry) if entry.binding == *binding => Msg::Present { revision: entry.revision, result: entry.result },
            Some(_) => Msg::Refused(Refusal::Conflict),
            None => Msg::Absent,
        }
    }

    pub fn commit(&mut self, epoch: u64, from: Sender, expected_revision: u64, binding: &Binding, new_state: u64) -> Msg {
        if !self.is_writer(epoch, from) {
            return Msg::Refused(Refusal::StaleEpoch);
        }
        let Some(current) = self.committed.filter(Committed::intact) else {
            return Msg::Refused(Refusal::RecoveryFailed);
        };
        if expected_revision != current.revision {
            return Msg::Refused(Refusal::RevisionMismatch);
        }
        match self.find(binding) {
            Some(entry) if entry.binding == *binding => return Msg::Present { revision: entry.revision, result: entry.result },
            Some(_) => return Msg::Refused(Refusal::Conflict),
            None => {}
        }
        if apply(binding.method, current.counter, binding.payload) != Some(new_state) {
            return Msg::Refused(Refusal::InvalidMutation);
        }
        let mine = self.ledger.iter().filter(|e| same_principal(&e.binding, binding)).count();
        if mine >= MAX_PER_PRINCIPAL || self.ledger.len() >= MAX_ENTRIES || self.ledger.try_reserve(1).is_err() {
            return Msg::Refused(Refusal::QuotaExceeded);
        }
        let next = Committed::new(current.schema, current.revision + 1, new_state);
        self.ledger.push(Entry { binding: *binding, result: new_state, revision: next.revision });
        self.committed = Some(next);
        Msg::Committed { revision: next.revision, result: new_state }
    }

    /// Fault injection replaces the stored state; it never touches the ledger.
    pub fn replace_committed(&mut self, state: Option<Committed>) {
        self.committed = state;
    }

    fn find(&self, binding: &Binding) -> Option<&Entry> {
        self.ledger.iter().find(|e| same_principal(&e.binding, binding) && e.binding.request_id == binding.request_id)
    }
}

impl Default for Authority {
    fn default() -> Self { Self::new() }
}

fn same_principal(a: &Binding, b: &Binding) -> bool {
    a.caller_cell == b.caller_cell && a.caller_generation == b.caller_generation
}
