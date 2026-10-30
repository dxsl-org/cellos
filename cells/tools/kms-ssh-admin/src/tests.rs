//! Host tests of the admin cell (k7-cp1-draft §6): armor, chunk driver, delete, output,
//! error mapping, class guard and the real-service integration. Shared fixtures and
//! doubles live here.
//!
//! Fixtures are synthetic: containers built from the public RFC 8032 section 7.1 TEST 1
//! key pair by a local encoder. No key is generated and no OpenSSH reference blob is
//! copied into the repository.

use core::cell::{Cell, RefCell};
use std::string::String;
use std::vec::Vec;

use ostd::clients::kms::{
    KmsClientError, KmsCustodyPosture, KmsSshKeyCustody, SshChunkAckPayload, SshKeyStatusPayload,
    SshTransferBeginResponsePayload, KMS_SSH_CHUNK_LEN,
};
use terminal_sdk::input::EventKind;

use crate::console::{Block, Console, Events, Next, Word};
use crate::kms::Kms;
use crate::secret::Secret;

mod armor;
mod class_guard;
mod console;
mod delete;
mod driver;
mod mapping;
mod output;
mod real;

pub const ENTER: u32 = 0x110001;
pub const ESCAPE: u32 = 0x110002;
pub const BACKSPACE: u32 = 0x110003;

/// RFC 8032 section 7.1 TEST 1: public key of the seed `9d61b19d...`.
pub const PUBLIC: [u8; 32] = [
    0xd7, 0x5a, 0x98, 0x01, 0x82, 0xb1, 0x0a, 0xb7, 0xd5, 0x4b, 0xfe, 0xd3, 0xc9, 0x64, 0x07, 0x3a,
    0x0e, 0xe1, 0x72, 0xf3, 0xda, 0xa6, 0x23, 0x25, 0xaf, 0x02, 0x1a, 0x68, 0xf7, 0x07, 0x51, 0x1a,
];
/// RFC 8032 section 7.1 TEST 1 secret key.
pub const SEED: [u8; 32] = [
    0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c, 0xc4,
    0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae, 0x7f, 0x60,
];

fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn put_string(out: &mut Vec<u8>, bytes: &[u8]) {
    put_u32(out, bytes.len() as u32);
    out.extend_from_slice(bytes);
}

pub fn public_blob() -> Vec<u8> {
    let mut blob = Vec::new();
    put_string(&mut blob, b"ssh-ed25519");
    put_string(&mut blob, &PUBLIC);
    blob
}

/// A plain (`none`/`none`) OpenSSH v1 container; `comment_len` pads the blob to any size.
pub fn plain_blob(comment_len: usize) -> Vec<u8> {
    let mut inner = Vec::new();
    put_u32(&mut inner, 0x0102_0304);
    put_u32(&mut inner, 0x0102_0304);
    put_string(&mut inner, b"ssh-ed25519");
    put_string(&mut inner, &PUBLIC);
    let mut keypair = SEED.to_vec();
    keypair.extend_from_slice(&PUBLIC);
    put_string(&mut inner, &keypair);
    put_string(&mut inner, &std::vec![b'c'; comment_len]);
    let mut pad = 1u8;
    while inner.len() % 8 != 0 {
        inner.push(pad);
        pad += 1;
    }
    container(b"none", b"none", &[], &inner)
}

/// A passphrase-wrapped container with an arbitrary 160-byte "ciphertext": enough for
/// KMS's structural checks (no KDF runs at import).
pub fn wrapped_blob() -> Vec<u8> {
    let mut options = Vec::new();
    put_string(&mut options, &[7u8; 16]);
    put_u32(&mut options, 16);
    let ciphertext: Vec<u8> = (0..160u32).map(|i| (i * 7 + 3) as u8).collect();
    container(b"aes256-ctr", b"bcrypt", &options, &ciphertext)
}

fn container(cipher: &[u8], kdf: &[u8], options: &[u8], private: &[u8]) -> Vec<u8> {
    let mut blob = b"openssh-key-v1\0".to_vec();
    put_string(&mut blob, cipher);
    put_string(&mut blob, kdf);
    put_string(&mut blob, options);
    put_u32(&mut blob, 1);
    put_string(&mut blob, &public_blob());
    put_string(&mut blob, private);
    blob
}

/// A deterministic pseudo-random blob (not an OpenSSH container) of `len` bytes, for
/// chunking tests where KMS is a fake.
pub fn filler(len: usize) -> Vec<u8> {
    let mut state = 0x9e37_79b9u32;
    (0..len)
        .map(|_| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 24) as u8
        })
        .collect()
}

pub fn status(
    custody: KmsSshKeyCustody,
    posture: KmsCustodyPosture,
    fingerprint: [u8; 32],
) -> SshKeyStatusPayload {
    SshKeyStatusPayload {
        algorithm: types::kms::KmsKeyAlgorithm::SshEd25519,
        readiness: types::kms::KmsCapabilityReadiness::Ready,
        provider: types::kms::KmsProviderKind::None,
        posture,
        key_present: 1,
        custody,
        key_count: 1,
        store_revision: 1,
        public_key: PUBLIC,
        fingerprint_sha256: fingerprint,
    }
}

// ---------------------------------------------------------------------------------------
// Scripted console

pub fn text(value: &str) -> EventKind {
    EventKind::Text(String::from(value))
}

pub fn key(code: u32) -> EventKind {
    EventKind::Key {
        code,
        modifiers: 0,
        pressed: 1,
    }
}

/// Events from a script; an exhausted script is a lost session.
pub struct Script {
    events: std::collections::VecDeque<EventKind>,
}

impl Script {
    pub fn new(events: Vec<EventKind>) -> Self {
        Self {
            events: events.into(),
        }
    }

    pub fn remaining(&self) -> usize {
        self.events.len()
    }
}

impl Events for Script {
    fn next(&mut self) -> Next {
        match self.events.pop_front() {
            Some(event) => Next::Event(event),
            None => Next::Lost,
        }
    }
}

/// Typed events for `text`: the characters as one `Text` event per line plus Enter.
pub fn typed_lines(text_block: &str) -> Vec<EventKind> {
    let mut events = Vec::new();
    for line in text_block.lines() {
        events.push(text(line));
        events.push(key(ENTER));
    }
    events
}

/// A paste of the whole text as one bracketed paste.
pub fn pasted(text_block: &str) -> Vec<EventKind> {
    std::vec![EventKind::PasteBegin, text(text_block), EventKind::PasteEnd]
}

pub struct TestConsole {
    pub out: String,
    pub err: String,
    block: Vec<EventKind>,
    word: Vec<EventKind>,
    pub lost: bool,
    pub block_reads: usize,
    pub word_reads: usize,
    /// What the block reader left in its buffer on return (for the wipe checks).
    pub block_outcome: Option<Block>,
}

impl TestConsole {
    pub fn new() -> Self {
        Self {
            out: String::new(),
            err: String::new(),
            block: Vec::new(),
            word: Vec::new(),
            lost: false,
            block_reads: 0,
            word_reads: 0,
            block_outcome: None,
        }
    }

    pub fn with_block(mut self, events: Vec<EventKind>) -> Self {
        self.block = events;
        self
    }

    pub fn with_word(mut self, events: Vec<EventKind>) -> Self {
        self.word = events;
        self
    }
}

impl Console for TestConsole {
    fn out(&mut self, text: &str) {
        self.out.push_str(text);
    }

    fn err(&mut self, text: &str) {
        self.err.push_str(text);
    }

    fn read_key_block(&mut self, buf: &mut Secret) -> Block {
        self.block_reads += 1;
        let mut script = Script::new(core::mem::take(&mut self.block));
        let outcome = crate::console::read_block(&mut script, buf);
        if outcome == Block::Lost {
            self.lost = true;
        }
        self.block_outcome = Some(outcome);
        outcome
    }

    fn read_word(&mut self) -> Word {
        self.word_reads += 1;
        let mut script = Script::new(core::mem::take(&mut self.word));
        let word = crate::console::read_word(&mut script);
        if word == Word::Lost {
            self.lost = true;
        }
        word
    }

    fn lost(&self) -> bool {
        self.lost
    }
}

// ---------------------------------------------------------------------------------------
// Scripted key service

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    Status(Vec<u8>),
    Begin {
        name: Vec<u8>,
        total: u32,
        sha: [u8; 32],
    },
    Chunk {
        handle: u64,
        index: u32,
        data: Vec<u8>,
    },
    Commit {
        handle: u64,
        count: u32,
        sha: [u8; 32],
    },
    Abort(u64),
    Delete {
        name: Vec<u8>,
        public_key: [u8; 32],
    },
}

/// Which call an injected failure or reply deviation applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum At {
    Begin,
    Chunk(u32),
    Commit,
    Status,
    Delete,
}

/// A deviation in a reply that is otherwise well-formed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tamper {
    ZeroHandle,
    BeginLength,
    BeginCount,
    BeginCapacity,
    AckHandle,
    AckNext,
    AckReceived,
    CommitNoKey,
}

pub const HANDLE: u64 = 0x5eed;

pub struct FakeKms {
    pub calls: RefCell<Vec<Call>>,
    pub fail: Cell<Option<(At, KmsClientError)>>,
    pub tamper: Cell<Option<Tamper>>,
    pub stored: RefCell<Option<SshKeyStatusPayload>>,
    pub commit_status: SshKeyStatusPayload,
    received: Cell<u32>,
    total: Cell<u32>,
}

impl FakeKms {
    pub fn new() -> Self {
        Self {
            calls: RefCell::new(Vec::new()),
            fail: Cell::new(None),
            tamper: Cell::new(None),
            stored: RefCell::new(None),
            commit_status: status(
                KmsSshKeyCustody::Plain,
                KmsCustodyPosture::DevPostureVolatile,
                [0xAB; 32],
            ),
            received: Cell::new(0),
            total: Cell::new(0),
        }
    }

    pub fn failing(at: At, error: KmsClientError) -> Self {
        let kms = Self::new();
        kms.fail.set(Some((at, error)));
        kms
    }

    pub fn tampered(tamper: Tamper) -> Self {
        let kms = Self::new();
        kms.tamper.set(Some(tamper));
        kms
    }

    pub fn with_stored(self, stored: SshKeyStatusPayload) -> Self {
        *self.stored.borrow_mut() = Some(stored);
        self
    }

    pub fn calls(&self) -> Vec<Call> {
        self.calls.borrow().clone()
    }

    fn check(&self, at: At) -> Result<(), KmsClientError> {
        match self.fail.get() {
            Some((wanted, error)) if wanted == at => Err(error),
            _ => Ok(()),
        }
    }

    fn tampered_with(&self, tamper: Tamper) -> bool {
        self.tamper.get() == Some(tamper)
    }
}

impl Kms for FakeKms {
    fn status(&self, name: &[u8]) -> Result<SshKeyStatusPayload, KmsClientError> {
        self.calls.borrow_mut().push(Call::Status(name.to_vec()));
        self.check(At::Status)?;
        match *self.stored.borrow() {
            Some(stored) => Ok(stored),
            None => Err(KmsClientError::Service(
                ostd::clients::kms::KmsErrorCode::SshKeyNotFound,
            )),
        }
    }

    fn import_begin(
        &self,
        name: &[u8],
        total_len: u32,
        blob_sha256: &[u8; 32],
    ) -> Result<SshTransferBeginResponsePayload, KmsClientError> {
        self.calls.borrow_mut().push(Call::Begin {
            name: name.to_vec(),
            total: total_len,
            sha: *blob_sha256,
        });
        self.check(At::Begin)?;
        self.received.set(0);
        self.total.set(total_len);
        let mut reply = SshTransferBeginResponsePayload {
            handle: HANDLE,
            total_len,
            chunk_count: total_len.div_ceil(KMS_SSH_CHUNK_LEN as u32),
            chunk_capacity: KMS_SSH_CHUNK_LEN as u16,
            reserved: [0; 6],
        };
        if self.tampered_with(Tamper::ZeroHandle) {
            reply.handle = 0;
        }
        if self.tampered_with(Tamper::BeginLength) {
            reply.total_len += 1;
        }
        if self.tampered_with(Tamper::BeginCount) {
            reply.chunk_count += 1;
        }
        if self.tampered_with(Tamper::BeginCapacity) {
            reply.chunk_capacity = 100;
        }
        Ok(reply)
    }

    fn import_chunk(
        &self,
        handle: u64,
        index: u32,
        data: &[u8],
    ) -> Result<SshChunkAckPayload, KmsClientError> {
        self.calls.borrow_mut().push(Call::Chunk {
            handle,
            index,
            data: data.to_vec(),
        });
        self.check(At::Chunk(index))?;
        self.received.set(self.received.get() + data.len() as u32);
        let mut ack = SshChunkAckPayload {
            handle,
            next_chunk_index: index + 1,
            received_len: self.received.get(),
        };
        if self.tampered_with(Tamper::AckHandle) {
            ack.handle ^= 1;
        }
        if self.tampered_with(Tamper::AckNext) {
            ack.next_chunk_index += 1;
        }
        if self.tampered_with(Tamper::AckReceived) {
            ack.received_len += 1;
        }
        Ok(ack)
    }

    fn import_commit(
        &self,
        handle: u64,
        chunk_count: u32,
        blob_sha256: &[u8; 32],
    ) -> Result<SshKeyStatusPayload, KmsClientError> {
        self.calls.borrow_mut().push(Call::Commit {
            handle,
            count: chunk_count,
            sha: *blob_sha256,
        });
        self.check(At::Commit)?;
        let mut stored = self.commit_status;
        if self.tampered_with(Tamper::CommitNoKey) {
            stored.key_present = 0;
        }
        Ok(stored)
    }

    fn import_abort(&self, handle: u64) -> Result<(), KmsClientError> {
        self.calls.borrow_mut().push(Call::Abort(handle));
        Ok(())
    }

    fn delete(&self, name: &[u8], public_key: &[u8; 32]) -> Result<(), KmsClientError> {
        self.calls.borrow_mut().push(Call::Delete {
            name: name.to_vec(),
            public_key: *public_key,
        });
        self.check(At::Delete)?;
        Ok(())
    }
}

/// `connect` for `run`: a reference to the fake as the `Kms` implementation.
pub struct Ref<'a, K: Kms>(pub &'a K);

impl<K: Kms> Kms for Ref<'_, K> {
    fn status(&self, name: &[u8]) -> Result<SshKeyStatusPayload, KmsClientError> {
        self.0.status(name)
    }
    fn import_begin(
        &self,
        name: &[u8],
        total_len: u32,
        sha: &[u8; 32],
    ) -> Result<SshTransferBeginResponsePayload, KmsClientError> {
        self.0.import_begin(name, total_len, sha)
    }
    fn import_chunk(
        &self,
        handle: u64,
        index: u32,
        data: &[u8],
    ) -> Result<SshChunkAckPayload, KmsClientError> {
        self.0.import_chunk(handle, index, data)
    }
    fn import_commit(
        &self,
        handle: u64,
        count: u32,
        sha: &[u8; 32],
    ) -> Result<SshKeyStatusPayload, KmsClientError> {
        self.0.import_commit(handle, count, sha)
    }
    fn import_abort(&self, handle: u64) -> Result<(), KmsClientError> {
        self.0.import_abort(handle)
    }
    fn delete(&self, name: &[u8], public_key: &[u8; 32]) -> Result<(), KmsClientError> {
        self.0.delete(name, public_key)
    }
}

/// The armored text of `blob` as `ssh-keygen` writes it (70 columns, LF).
pub fn armor_text(blob: &[u8]) -> String {
    crate::armor::encode(blob, 70, "\n")
}

/// Run the whole cell against `kms` with the given console; returns the exit code.
pub fn run_cell<K: Kms>(args: &[&str], console: &mut TestConsole, kms: &K) -> i32 {
    crate::run(args, console, || Ok(Ref(kms)))
}
