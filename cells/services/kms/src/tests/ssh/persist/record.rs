//! Record v2: golden bytes, round trip, every bound, every identity field, and
//! the MAC covering every byte.

use std::vec;
use std::vec::Vec;

use super::*;
use crate::ssh::parser::parse_outer;
use crate::ssh::persist::devroot::mac_key;
use crate::ssh::persist::record::{
    digest, encode_present, encode_tombstone, identity, mac, meta, parse, CT_MIN, CUSTODY_WRAPPED,
    HEADER_LEN, KIND_SSH, MAC_LEN, MAGIC, STATE_PRESENT, STATE_TOMBSTONE, VERSION,
};
use crate::ssh::store::StoredIdentity;
use crate::storage::{JournalKey, JournalRecord, SlotId};

/// Present record: slot A, revision 1, no predecessor, name `golden`, the
/// public OpenSSH `aes256-ctr` fixture (rounds 16, salt 16, ct 160). Computed
/// independently in Python from the public dev root constant:
/// `hashlib.blake2s(record_without_mac, key=blake2s(DEV_ROOT_LABEL))`.
const GOLDEN_PRESENT: &str = concat!(
    "434b4d530200010101000000000000000000000000000000000000000000000000000000000000000000000000000000",
    "06676f6c64656e0000000000000000000000000000000000000000000000000000000000000000000000000000000000",
    "000000000000000000000000000000000002b33eaef37ea2df7caa010defdea34e241f65f1b529a4f43ed14327f5c54a",
    "ab6210000000104a1fdeae8d6ba607afd69d334f8d379a00000000000000000000000000000000000000000000000000",
    "0000000000000000000000000000000000000000000000a0005cef6efc85321022c013971749a151469c3d408c6578af",
    "1b6ba9d9c8449c0c356fa04b44fc966d8e5c0b69fd8200f88af275cd2b6ceac0608f289740045ecab3f7e66c98bd3fce",
    "201fcda7064cff120937b307494ce5917e1a741ae06b77fb192e2ebf80a13a8c42e17b04e47b31b46b2ad57f23cea2e5",
    "65398e73191c12779841ec6b51c6d03f4a3e80be5a28af1c4208889577836d18ea85ce3e878105ea5698cdb0ad4c4143",
    "6a2bfb9785a7493ac9bb8ad61d7074ed4aeb92bbccdb64a302",
);
const GOLDEN_TOMBSTONE: &str = concat!(
    "434b4d530201010002000000000000008e86847756551d61e0749ddb92ea15db032032e2d054311a0ec34cccd3210141",
    "000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
    "000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
    "000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
    "00000000000000000000000000000000000000000000000000def7d5addced45376f96217c53dfdd8fc4fe918f050b85",
    "82a15cbaefd247bfa2",
);
const GOLDEN_KEY: &str = "e6ee4e76ccf6c77fd5922dbeeecf818b9d7a34f58e679700c3c757749c9c16ce";
const GOLDEN_PRESENT_DIGEST: &str =
    "8e86847756551d61e0749ddb92ea15db032032e2d054311a0ec34cccd3210141";
const GOLDEN_TOMBSTONE_DIGEST: &str =
    "03196182986a66da13cc5a8fba52c6f3b0a5b1892bc64ab13ca7251d2aacc1fd";

/// Every field of a record, so a test can break exactly one. `bytes()` always
/// carries a correct MAC over whatever the fields say.
#[derive(Clone)]
pub(super) struct Raw {
    pub magic: [u8; 4],
    pub version: u8,
    pub slot: u8,
    pub kind: u8,
    pub state: u8,
    pub revision: u64,
    pub previous: [u8; 32],
    pub name_len: u8,
    pub name: [u8; 64],
    pub custody: u8,
    pub public: [u8; 32],
    pub rounds: u32,
    pub salt_len: u8,
    pub salt: [u8; 64],
    pub ct_len: u16,
    pub ct: Vec<u8>,
}

impl Raw {
    pub(super) fn present(slot: Slot) -> Self {
        let mut name = [0u8; 64];
        name[..2].copy_from_slice(NAME);
        let mut salt = [0u8; 64];
        salt[..16].copy_from_slice(&[0x11; 16]);
        Self {
            magic: MAGIC,
            version: VERSION,
            slot: slot.id(),
            kind: KIND_SSH,
            state: STATE_PRESENT,
            revision: 1,
            previous: [0; 32],
            name_len: 2,
            name,
            custody: CUSTODY_WRAPPED,
            public: cheap_public(),
            rounds: 1,
            salt_len: 16,
            salt,
            ct_len: 144,
            ct: vec![0xAB; 144],
        }
    }

    pub(super) fn tombstone(slot: Slot) -> Self {
        Self {
            state: STATE_TOMBSTONE,
            name_len: 0,
            name: [0; 64],
            custody: 0,
            public: [0; 32],
            rounds: 0,
            salt_len: 0,
            salt: [0; 64],
            ct_len: 0,
            ct: Vec::new(),
            ..Self::present(slot)
        }
    }

    pub(super) fn with_ct(mut self, ct: Vec<u8>) -> Self {
        self.ct_len = ct.len() as u16;
        self.ct = ct;
        self
    }

    /// Fields as written, MAC appended.
    pub(super) fn bytes(&self) -> Vec<u8> {
        let mut out = self.body();
        let tag = mac(&out);
        out.extend_from_slice(&tag);
        out
    }

    pub(super) fn body(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&self.magic);
        out.extend_from_slice(&[self.version, self.slot, self.kind, self.state]);
        out.extend_from_slice(&self.revision.to_le_bytes());
        out.extend_from_slice(&self.previous);
        out.push(self.name_len);
        out.extend_from_slice(&self.name);
        out.push(self.custody);
        out.extend_from_slice(&self.public);
        out.extend_from_slice(&self.rounds.to_le_bytes());
        out.push(self.salt_len);
        out.extend_from_slice(&self.salt);
        out.extend_from_slice(&self.ct_len.to_le_bytes());
        out.extend_from_slice(&self.ct);
        out
    }
}

fn accepts(slot: Slot, raw: &Raw) -> bool {
    parse(slot, &raw.bytes()).is_some()
}

fn fixture_identity() -> StoredIdentity {
    let blob = id_ed25519_enc();
    let outer = parse_outer(&blob).unwrap();
    StoredIdentity::wrapped(
        b"golden",
        outer.public,
        outer.salt,
        outer.rounds,
        outer.private,
    )
    .unwrap()
}

#[test]
fn golden_records_encode_and_decode_byte_for_byte() {
    assert_eq!(hex(GOLDEN_KEY), mac_key());
    let identity = fixture_identity();
    let mut out = [0u8; RECORD_MAX];
    let len = encode_present(Slot::A, &identity.record_view(1), &[0; 32], &mut out).unwrap();
    assert_eq!(len, 409);
    assert_eq!(out[..len], hex(GOLDEN_PRESENT)[..]);
    assert_eq!(digest(&out[..len]), hex32(GOLDEN_PRESENT_DIGEST));

    let parsed = parse(Slot::A, &out[..len]).unwrap();
    assert!(parsed.present);
    assert_eq!(parsed.revision, 1);
    assert_eq!(parsed.previous, [0; 32]);
    assert_eq!(parsed.name, b"golden");
    assert_eq!(parsed.public_key, id_ed25519_public());
    assert_eq!(parsed.rounds, 16);
    assert_eq!(parsed.salt.len(), 16);
    assert_eq!(parsed.ciphertext.len(), 160);

    // The tombstone chains onto it.
    let mut tomb = [0u8; RECORD_MAX];
    let tomb_len = encode_tombstone(Slot::B, 2, &hex32(GOLDEN_PRESENT_DIGEST), &mut tomb).unwrap();
    assert_eq!(tomb_len, HEADER_LEN + MAC_LEN);
    assert_eq!(tomb_len, 249);
    assert_eq!(tomb[..tomb_len], hex(GOLDEN_TOMBSTONE)[..]);
    assert_eq!(digest(&tomb[..tomb_len]), hex32(GOLDEN_TOMBSTONE_DIGEST));
    let meta = meta(Slot::B, &tomb[..tomb_len]).unwrap();
    assert!(!meta.present);
    assert_eq!(meta.revision, 2);
    assert_eq!(meta.previous, hex32(GOLDEN_PRESENT_DIGEST));
    assert_eq!(meta.digest, hex32(GOLDEN_TOMBSTONE_DIGEST));
}

#[test]
fn golden_hex_is_the_python_derivation_of_the_layout() {
    // Cross-check the golden against the layout by hand: header fields at
    // their documented offsets, MAC over everything before it.
    let golden = hex(GOLDEN_PRESENT);
    assert_eq!(&golden[..4], b"CKMS");
    assert_eq!(&golden[4..8], &[2, 0, 1, 1]);
    assert_eq!(&golden[8..16], &1u64.to_le_bytes());
    assert_eq!(golden[48], 6);
    assert_eq!(&golden[49..55], b"golden");
    assert_eq!(golden[113], 2);
    assert_eq!(&golden[114..146], &id_ed25519_public());
    assert_eq!(&golden[146..150], &16u32.to_le_bytes());
    assert_eq!(golden[150], 16);
    assert_eq!(&golden[215..217], &160u16.to_le_bytes());
    let (body, tag) = golden.split_at(golden.len() - 32);
    assert_eq!(mac(body), tag);
}

#[test]
fn round_trip_preserves_every_field() {
    let original = fixture_identity();
    let mut out = [0u8; RECORD_MAX];
    let len = encode_present(Slot::B, &original.record_view(9), &[7; 32], &mut out).unwrap();
    let decoded = identity(Slot::B, &out[..len]).unwrap();
    assert_eq!(decoded.name(), original.name());
    assert_eq!(decoded.public_key(), original.public_key());
    let (a, b) = (original.record_view(9), decoded.record_view(9));
    match (a.secret, b.secret) {
        (
            crate::ssh::store::SecretView::Wrapped {
                salt: sa,
                rounds: ra,
                ciphertext: ca,
            },
            crate::ssh::store::SecretView::Wrapped {
                salt: sb,
                rounds: rb,
                ciphertext: cb,
            },
        ) => assert_eq!((sa, ra, ca), (sb, rb, cb)),
        _ => panic!("wrapped on both sides"),
    }
    assert_eq!(parse(Slot::B, &out[..len]).unwrap().previous, [7; 32]);
    // A tombstone has no identity.
    let mut tomb = [0u8; RECORD_MAX];
    let tomb_len = encode_tombstone(Slot::A, 3, &[1; 32], &mut tomb).unwrap();
    assert!(identity(Slot::A, &tomb[..tomb_len]).is_none());
}

#[test]
fn the_encoder_refuses_what_the_decoder_would_reject() {
    let mut out = [0u8; RECORD_MAX];
    // Plain: no record ever holds a seed.
    let plain = StoredIdentity::plain(b"pk", public_of(&RFC8032_SEED_1), &RFC8032_SEED_1).unwrap();
    assert!(encode_present(Slot::A, &plain.record_view(1), &[0; 32], &mut out).is_none());
    // Revision 0 is never written.
    let identity = fixture_identity();
    assert!(encode_present(Slot::A, &identity.record_view(0), &[0; 32], &mut out).is_none());
    assert!(encode_tombstone(Slot::A, 0, &[0; 32], &mut out).is_none());
}

#[test]
fn ciphertext_length_bounds() {
    for (len, ok) in [
        (0usize, false),
        (16, false),
        (128, false),
        (143, false),
        (144, true),
        (160, true),
        (2_032, true),
        (2_048, true),
        (2_064, false),
        (145, false),
        (151, false),
        (2_049, false),
    ] {
        let raw = Raw::present(Slot::A).with_ct(vec![0x5A; len]);
        assert_eq!(accepts(Slot::A, &raw), ok, "ct_len {len}");
    }
    assert_eq!(CT_MIN, 144);
}

#[test]
fn ciphertext_length_field_must_match_the_file_length() {
    let raw = Raw::present(Slot::A);
    for declared in [0u16, 143, 145, 160, 2_048, u16::MAX] {
        let mut bad = raw.clone();
        bad.ct_len = declared;
        assert!(!accepts(Slot::A, &bad), "declared {declared}");
    }
    // Trailing byte after the MAC, missing byte, extra byte before it.
    let mut bytes = raw.bytes();
    bytes.push(0);
    assert!(parse(Slot::A, &bytes).is_none());
    let short = &raw.bytes()[..raw.bytes().len() - 1];
    assert!(parse(Slot::A, short).is_none());
}

#[test]
fn kdf_rounds_and_salt_bounds() {
    for (rounds, ok) in [
        (0u32, false),
        (1, true),
        (16, true),
        (64, true),
        (65, false),
        (u32::MAX, false),
    ] {
        let mut raw = Raw::present(Slot::A);
        raw.rounds = rounds;
        assert_eq!(accepts(Slot::A, &raw), ok, "rounds {rounds}");
    }
    for (len, ok) in [
        (0usize, false),
        (15, false),
        (16, true),
        (32, true),
        (64, true),
        (65, false),
    ] {
        let mut raw = Raw::present(Slot::A);
        raw.salt = [0; 64];
        for byte in raw.salt.iter_mut().take(len.min(64)) {
            *byte = 0x11;
        }
        raw.salt_len = len as u8;
        assert_eq!(accepts(Slot::A, &raw), ok, "salt {len}");
    }
}

#[test]
fn custody_state_and_name_rules() {
    for (custody, ok) in [
        (0u8, false),
        (1, false),
        (2, true),
        (3, false),
        (255, false),
    ] {
        let mut raw = Raw::present(Slot::A);
        raw.custody = custody;
        assert_eq!(accepts(Slot::A, &raw), ok, "custody {custody}");
    }
    for (state, ok) in [(0u8, false), (1, true), (2, false), (255, false)] {
        // State 0 with a populated body is not a tombstone.
        let mut raw = Raw::present(Slot::A);
        raw.state = state;
        assert_eq!(accepts(Slot::A, &raw), ok, "state {state}");
    }
    // Name length and charset.
    for (name, ok) in [
        (&b""[..], false),
        (&b"a"[..], true),
        (&b"Key_9-x"[..], true),
        (&[b'a'; 64][..], true),
        (&b"bad name"[..], false),
        (&b"bad.name"[..], false),
        (&b"caf\xc3\xa9"[..], false),
        (&b"nul\0x"[..], false),
    ] {
        let mut raw = Raw::present(Slot::A);
        raw.name = [0; 64];
        raw.name[..name.len()].copy_from_slice(name);
        raw.name_len = name.len() as u8;
        assert_eq!(accepts(Slot::A, &raw), ok, "name {name:?}");
    }
    let mut oversize = Raw::present(Slot::A);
    oversize.name_len = 65;
    assert!(!accepts(Slot::A, &oversize));
    // Revision 0 is invalid.
    let mut zero = Raw::present(Slot::A);
    zero.revision = 0;
    assert!(!accepts(Slot::A, &zero));
}

#[test]
fn nonzero_padding_is_refused() {
    for at in [2usize, 10, 63] {
        let mut raw = Raw::present(Slot::A);
        raw.name[at] = 1;
        assert!(!accepts(Slot::A, &raw), "name pad {at}");
    }
    for at in [16usize, 17, 40, 63] {
        let mut raw = Raw::present(Slot::A);
        raw.salt[at] = 1;
        assert!(!accepts(Slot::A, &raw), "salt pad {at}");
    }
}

#[test]
fn tombstones_are_all_zero_beyond_the_header() {
    assert!(accepts(Slot::B, &Raw::tombstone(Slot::B)));
    type Break = (&'static str, fn(&mut Raw));
    let breaks: [Break; 8] = [
        ("name_len", |raw| raw.name_len = 1),
        ("name", |raw| raw.name[0] = b'a'),
        ("custody", |raw| raw.custody = 2),
        ("public", |raw| raw.public[31] = 1),
        ("rounds", |raw| raw.rounds = 1),
        ("salt_len", |raw| raw.salt_len = 16),
        ("salt", |raw| raw.salt[0] = 1),
        ("ct", |raw| {
            raw.ct = vec![0; 144];
            raw.ct_len = 144;
        }),
    ];
    for (what, mutate) in breaks {
        let mut raw = Raw::tombstone(Slot::B);
        mutate(&mut raw);
        assert!(!accepts(Slot::B, &raw), "tombstone with {what}");
    }
}

#[test]
fn wrong_magic_version_slot_and_kind_are_invalid() {
    assert!(accepts(Slot::A, &Raw::present(Slot::A)));
    let mut magic = Raw::present(Slot::A);
    magic.magic = *b"CKMT";
    assert!(!accepts(Slot::A, &magic));
    for version in [0u8, 1, 3, 255] {
        let mut raw = Raw::present(Slot::A);
        raw.version = version;
        assert!(!accepts(Slot::A, &raw), "version {version}");
    }
    // A record for slot A is invalid as the content of slot B, and back.
    assert!(!accepts(Slot::B, &Raw::present(Slot::A)));
    assert!(!accepts(Slot::A, &Raw::present(Slot::B)));
    let mut bad_slot = Raw::present(Slot::A);
    bad_slot.slot = 2;
    assert!(!accepts(Slot::A, &bad_slot));
    assert!(!accepts(Slot::B, &bad_slot));
    for kind in [0u8, 2, 255] {
        let mut raw = Raw::present(Slot::A);
        raw.kind = kind;
        assert!(!accepts(Slot::A, &raw), "kind {kind}");
    }
}

#[test]
fn a_v1_relay_record_is_never_an_ssh_record_and_back() {
    // The same key, so only the format rejects.
    let key: JournalKey = mac_key();
    let relay = JournalRecord::placeholder(SlotId::A, 1, 1, [0; 32]);
    let v1 = relay.encode(&key);
    assert_eq!(v1.len(), 186);
    assert!(parse(Slot::A, &v1).is_none());
    assert!(parse(Slot::B, &v1).is_none());
    // And a v2 record fed to the v1 decoder.
    let v2 = Raw::present(Slot::A).bytes();
    assert!(JournalRecord::decode(&v2, &key, SlotId::A).is_none());
    let tomb = Raw::tombstone(Slot::A).bytes();
    assert!(JournalRecord::decode(&tomb, &key, SlotId::A).is_none());
    // A v1 body relabelled as version 2 is still rejected (length, kind).
    let mut relabel = v1.to_vec();
    relabel[4] = 2;
    assert!(parse(Slot::A, &relabel).is_none());
}

#[test]
fn the_mac_covers_every_byte() {
    for raw in [Raw::present(Slot::A), Raw::tombstone(Slot::A)] {
        let good = raw.bytes();
        assert!(parse(Slot::A, &good).is_some());
        for at in 0..good.len() {
            for bit in [0x01u8, 0x80] {
                let mut flipped = good.clone();
                flipped[at] ^= bit;
                assert!(
                    parse(Slot::A, &flipped).is_none(),
                    "flip at {at} bit {bit:#x} of {} bytes",
                    good.len()
                );
            }
        }
    }
}

#[test]
fn a_wrong_key_mac_is_invalid() {
    let mut bytes = Raw::present(Slot::A).bytes();
    let body_len = bytes.len() - 32;
    let other = {
        use blake2::digest::{KeyInit, Mac};
        let mut hasher = <blake2::Blake2sMac256 as KeyInit>::new((&[7u8; 32]).into());
        Mac::update(&mut hasher, &bytes[..body_len]);
        hasher.finalize().into_bytes()
    };
    bytes[body_len..].copy_from_slice(&other);
    assert!(parse(Slot::A, &bytes).is_none());
}

#[test]
fn records_never_exceed_the_buffer() {
    let mut max = Raw::present(Slot::A).with_ct(vec![0x5A; 2_048]).bytes();
    assert_eq!(max.len(), RECORD_MAX);
    assert!(parse(Slot::A, &max).is_some());
    max.push(0);
    assert!(parse(Slot::A, &max).is_none());
    assert!(parse(Slot::A, &[]).is_none());
}
