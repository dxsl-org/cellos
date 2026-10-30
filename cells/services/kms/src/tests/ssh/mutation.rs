//! Always-on deterministic mutation test (xorshift64, fixed seed): the parser
//! never panics, `Ok` implies every outer bound, no mutation of any shape runs
//! the KDF at import, and a mutated blob never leaves a half-installed key.

use std::vec;
use std::vec::Vec;

use super::fixtures::*;
use super::*;
use crate::ssh::parser::{parse_outer, Cipher};
use crate::ssh::unwrap::{plain_seed, KDF_CALLS};

struct XorShift(u64);

impl XorShift {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }
}

const SPLICES: [u32; 4] = [0, 0xFFFF_FFFF, 0x7FFF_FFFF, 0x8000_0000];

fn mutate(rng: &mut XorShift, mut blob: Vec<u8>) -> Vec<u8> {
    for _ in 0..=rng.below(3) {
        if blob.is_empty() {
            blob.push(rng.next() as u8);
            continue;
        }
        match rng.below(6) {
            0 => {
                let at = rng.below(blob.len());
                blob[at] ^= 1 << rng.below(8);
            }
            1 => {
                let at = rng.below(blob.len());
                blob[at] = rng.next() as u8;
            }
            2 => blob.truncate(rng.below(blob.len() + 1)),
            3 => {
                for _ in 0..=rng.below(8) {
                    blob.push(rng.next() as u8);
                }
            }
            4 if blob.len() >= 4 => {
                let at = rng.below(blob.len() - 3);
                let value = SPLICES[rng.below(SPLICES.len())];
                blob[at..at + 4].copy_from_slice(&value.to_be_bytes());
            }
            _ if blob.len() > 96 => {
                // Duplicate one 96-byte chunk over another position.
                let from = rng.below(blob.len() - 95);
                let to = rng.below(blob.len() - 95);
                let chunk: Vec<u8> = blob[from..from + 96].to_vec();
                blob[to..to + 96].copy_from_slice(&chunk);
            }
            _ => {}
        }
    }
    blob
}

fn corpus() -> Vec<Vec<u8>> {
    vec![
        id_ed25519(),
        id_ed25519_enc(),
        puttygen_overpadded(),
        Craft::plain(&RFC8032_SEED_1).bytes(),
        Craft::wrapped_opaque(&[1u8; 16], 64, vec![0x77; 160]).bytes(),
        Craft::wrapped_opaque(&[1u8; 64], 1, vec![0x77; 2048]).bytes(),
    ]
}

#[test]
fn parser_mutation_never_panics_and_never_runs_the_kdf() {
    let corpus = corpus();
    let mut rng = XorShift(0x9E37_79B9_7F4A_7C15);
    let (mut ok, mut rejected) = (0usize, 0usize);
    let kdf_before = KDF_CALLS.with(|calls| calls.get());
    for _ in 0..200_000 {
        let base = corpus[rng.below(corpus.len())].clone();
        let blob = mutate(&mut rng, base);
        match parse_outer(&blob) {
            Ok(outer) => {
                ok += 1;
                assert!(blob.len() <= 65_536 + 1024);
                match outer.cipher {
                    Cipher::None => {
                        assert!(outer.private.len() % 8 == 0 && outer.private.len() >= 136);
                        assert!(outer.salt.is_empty() && outer.rounds == 0);
                        let _ = plain_seed(outer.private, Cipher::None, &outer.public);
                    }
                    Cipher::Aes256Ctr => {
                        assert!(outer.private.len() % 16 == 0);
                        assert!((144..=2048).contains(&outer.private.len()));
                        assert!((16..=64).contains(&outer.salt.len()));
                        assert!((1..=64).contains(&outer.rounds));
                    }
                }
            }
            Err(_) => rejected += 1,
        }
    }
    assert_eq!(
        KDF_CALLS.with(|calls| calls.get()),
        kdf_before,
        "no KDF at import, ever"
    );
    assert!(ok > 0 && rejected > 100_000, "ok {ok} rejected {rejected}");
}

#[test]
fn mutated_imports_never_leave_a_partial_key() {
    let corpus = corpus();
    let mut rng = XorShift(0x0123_4567_89AB_CDEF);
    let mut service = service();
    let admin = Actor::admin();
    let kdf_before = KDF_CALLS.with(|calls| calls.get());
    let mut installed = 0usize;
    for round in 0..1_500 {
        let base = corpus[rng.below(corpus.len())].clone();
        let blob = mutate(&mut rng, base);
        let name: &[u8] = b"fuzz";
        match import_blob(&mut service, &admin, name, &blob) {
            Ok(status) => {
                installed += 1;
                assert_eq!(status.key_count, 1);
                let identity = service.ssh.identity.as_ref().expect("installed");
                assert_eq!(identity.name(), name);
                delete_key(&mut service, &admin, name, status.public_key).unwrap();
            }
            Err(code) => {
                assert!(
                    matches!(
                        code,
                        KmsErrorCode::SshKeyFormatRejected
                            | KmsErrorCode::SshStoreFull
                            | KmsErrorCode::SshImportIntegrity
                            | KmsErrorCode::InvalidRequest // empty or over-long blob: Begin bounds
                    ),
                    "round {round}: {code:?}"
                );
                assert!(service.ssh.identity.is_none());
            }
        }
        assert!(service.ssh.arena.is_zeroed());
        assert!(matches!(
            service.ssh.import,
            crate::ssh::import::ImportSlot::Idle
        ));
    }
    assert_eq!(KDF_CALLS.with(|calls| calls.get()), kdf_before);
    assert!(
        installed > 0,
        "the unmutated or no-op mutations of valid blobs import"
    );
}
