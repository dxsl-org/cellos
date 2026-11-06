#![allow(dead_code)]

use cellos_fs::disk::{BlockDevice, MemDisk};
use durable_charge_codec::{decode, encode, Header, Limits};
use durable_charge_journal::{Journal, JournalExpectation};
use ed25519_compact::{KeyPair, Seed};

pub const LEDGER: [u8; 32] = [1; 32];
pub const GENESIS: [u8; 32] = [2; 32];
pub const EPOCH: u64 = 1;

pub fn limits() -> Limits {
    Limits {
        max_record_bytes: 65536,
        max_entries: 1024,
        max_records: 64,
        max_reference_depth: 64,
        max_total_bytes: 1 << 20,
        max_work: 1 << 24,
    }
}

pub fn writer() -> KeyPair {
    KeyPair::from_seed(Seed::new([7; 32]))
}

pub fn expect(min_sequence: u64) -> JournalExpectation {
    JournalExpectation {
        ledger_id: LEDGER,
        genesis_id: GENESIS,
        writer_public_key: *writer().pk,
        min_sequence,
    }
}

pub fn header(revision: u64, previous: [u8; 32]) -> Header {
    Header {
        journal_ledger: LEDGER,
        journal_genesis: GENESIS,
        obligation_id: [0; 16],
        adoption_id: [0; 16],
        local_transaction: [3; 16],
        local_revision: revision,
        journal_writer_epoch: EPOCH,
        previous_record_digest: previous,
    }
}

/// Kind 32 (vector manifest) with `entries` dimensions: 284 + 24*entries bytes.
pub fn encode_with(header: &Header, entries: usize) -> Vec<u8> {
    let mut payload = vec![1u8; 36 + 24 * entries];
    payload[32..36].copy_from_slice(&(entries as u32).to_le_bytes());
    for i in 0..entries {
        let at = 36 + 24 * i;
        payload[at..at + 2].copy_from_slice(&(i as u16 + 1).to_le_bytes());
        payload[at + 2..at + 4].copy_from_slice(&1u16.to_le_bytes());
        payload[at + 4..at + 8].fill(0);
        payload[at + 16..at + 24].copy_from_slice(&1u64.to_le_bytes());
    }
    let mut out = vec![0; payload.len() + 248];
    encode(32, header, &payload, &mut out, &limits()).expect("fixture encodes");
    out
}

pub fn record(revision: u64, previous: [u8; 32]) -> Vec<u8> {
    encode_with(&header(revision, previous), 1)
}

pub fn digest(bytes: &[u8]) -> [u8; 32] {
    decode(bytes, &limits()).expect("fixture decodes").digest
}

/// Raw disk access, as an attacker with write access to the medium would have.
pub fn block(disk: &MemDisk, index: u64) -> [u8; 4096] {
    let mut buf = [0u8; 4096];
    disk.clone().read_block(index, &mut buf).unwrap();
    buf
}

pub fn put(disk: &MemDisk, index: u64, buf: &[u8; 4096]) {
    disk.clone().write_block(index, buf).unwrap();
}

/// Formats and appends two records (blocks 2 and 3): slot 1 holds sequence 2,
/// slot 0 holds sequence 3.
pub fn two_record_journal() -> (MemDisk, Vec<u8>, Vec<u8>) {
    let disk = MemDisk::new(16);
    let keys = writer();
    let mut journal = Journal::format(disk.clone(), LEDGER, GENESIS, EPOCH, &keys).unwrap();
    let r1 = record(1, GENESIS);
    journal.append_record(&keys, &r1, &limits()).unwrap();
    let r2 = record(2, digest(&r1));
    journal.append_record(&keys, &r2, &limits()).unwrap();
    (disk, r1, r2)
}
