//! Nothing but the verbatim wrapped ciphertext (by design) reaches storage:
//! never the seed, a passphrase, a plain private section or the decrypted
//! inner section.

use std::format;
use std::string::String;
use std::vec::Vec;

use super::fixtures::*;
use super::*;
use crate::ssh::parser::{parse_outer, Cipher};
use crate::ssh::unwrap::plain_seed;

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

fn printable_forms(secret: &[u8]) -> Vec<String> {
    let mut forms = Vec::new();
    let hex: String = secret.iter().map(|byte| format!("{byte:02x}")).collect();
    forms.push(hex);
    forms.push(format!("{secret:?}"));
    if let Ok(text) = core::str::from_utf8(secret) {
        forms.push(text.into());
    }
    forms
}

struct Secrets(Vec<(&'static str, Vec<u8>)>);

fn secrets() -> Secrets {
    let seed = RFC8032_SEED_1;
    let public = public_of(&seed);
    let inner = inner_section(&seed, b"crafted", 16, Pad::Natural);
    let mut keypair = seed.to_vec();
    keypair.extend_from_slice(&public);
    let plain_blob = id_ed25519();
    let outer = parse_outer(&plain_blob).unwrap();
    let plain_seed_bytes = *plain_seed(outer.private, Cipher::None, &outer.public).unwrap();
    Secrets(std::vec![
        ("seed", seed.to_vec()),
        ("passphrase", PASS.to_vec()),
        ("wrong passphrase", b"nope-nope".to_vec()),
        ("decrypted inner section", inner),
        ("keypair half", keypair),
        ("plain fixture private section", outer.private.to_vec()),
        ("plain fixture seed", plain_seed_bytes.to_vec()),
    ])
}

/// import (wrapped) -> unlock -> sign -> failed unlock -> delete -> import
/// (plain) -> delete, over one leaf.
fn cycle() -> FileLeaf {
    let admin = Actor::admin();
    let login = Actor::login();
    let mut leaf = fresh();
    import_cheap(&mut leaf, &admin).unwrap();
    stage_secret(&mut leaf, &login, cheap_public(), PASS).unwrap();
    sign(&mut leaf, &login, cheap_public(), SESSION_ID, b"user").unwrap();
    stage_secret(&mut leaf, &login, cheap_public(), b"nope-nope").unwrap();
    let _ = sign(&mut leaf, &login, cheap_public(), SESSION_ID, b"user");
    delete_key(&mut leaf, &admin, NAME, cheap_public()).unwrap();
    import_blob(&mut leaf, &admin, b"pk", &id_ed25519()).unwrap();
    delete_key(&mut leaf, &admin, b"pk", id_ed25519_public()).unwrap();
    leaf
}

#[test]
fn no_secret_reaches_the_storage_fake() {
    let leaf = cycle();
    let io = &leaf.persist.io;
    assert_eq!(io.writes(), 2, "wrapped import and its tombstone only");
    let secrets = secrets();
    for (what, secret) in &secrets.0 {
        for payload in &io.received {
            assert!(!contains(payload, secret), "{what} written to storage");
        }
        for file in io.files.iter().flatten() {
            assert!(!contains(file, secret), "{what} stored");
        }
    }
    // Control: the verbatim ciphertext does appear, by design (it is
    // already encrypted under the user's passphrase).
    let blob = cheap_blob();
    let outer = parse_outer(&blob).unwrap();
    assert!(contains(&io.received[0], outer.private));
}

#[test]
fn the_scan_is_not_vacuous() {
    // A planted copy of every secret is found by the same scan.
    let secrets = secrets();
    for (what, secret) in &secrets.0 {
        let mut payload = std::vec![0xEE; 40];
        payload.extend_from_slice(secret);
        payload.extend_from_slice(&[0xEE; 40]);
        assert!(contains(&payload, secret), "{what}");
    }
}

#[test]
fn no_secret_in_debug_output() {
    let leaf = cycle();
    let mut texts = std::vec![format!("{leaf:?}"), format!("{:?}", leaf.persist)];
    let mut live = fresh();
    let admin = Actor::admin();
    import_cheap(&mut live, &admin).unwrap();
    texts.push(format!("{live:?}"));
    texts.push(format!("{:?}", live.persist));
    for text in &texts {
        for (what, secret) in &secrets().0 {
            for form in printable_forms(secret) {
                assert!(!text.contains(&form), "{what} in Debug output: {text}");
            }
        }
    }
}
