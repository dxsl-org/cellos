//! Host measurement of the bcrypt KDF (decision D-5.8). The target cost (QEMU
//! TCG, bare metal) is not measured here.
//!
//! Run optimized:
//! `cargo test --release --offline --locked -p service-kms --features
//! ssh-dev-custody --target x86_64-unknown-linux-gnu kdf_cost -- --ignored --nocapture`

use std::format;
use std::time::Instant;
use std::vec::Vec;

use super::fixtures::*;
use crate::ssh::unwrap::unwrap_seed;

#[test]
#[ignore = "measurement, not a correctness test; see the module doc for the command"]
fn kdf_cost_host_rounds_1_16_64() {
    let seed = RFC8032_SEED_1;
    let public = public_of(&seed);
    let salt = [0x44u8; 16];
    for rounds in [1u32, 16, 64] {
        let craft = Craft::wrapped(&seed, PASSPHRASE, &salt, rounds);
        let mut samples: Vec<f64> = Vec::new();
        for _ in 0..5 {
            let start = Instant::now();
            let recovered =
                unwrap_seed(PASSPHRASE, &salt, rounds, &craft.private, &public).unwrap();
            samples.push(start.elapsed().as_secs_f64() * 1000.0);
            assert_eq!(*recovered, seed);
        }
        samples.sort_by(f64::total_cmp);
        std::println!(
            "kdf_cost rounds={rounds} samples_ms={} min={:.2} median={:.2} max={:.2}",
            samples
                .iter()
                .map(|s| format!("{s:.2}"))
                .collect::<Vec<_>>()
                .join(","),
            samples[0],
            samples[2],
            samples[4]
        );
    }
}

#[test]
fn kdf_matches_the_openssh_reference_at_rounds_16() {
    // `ssh-keygen -a 16` container: passphrase `hunter42` recovers the seed
    // whose public key is the published one (independent oracle: python
    // `bcrypt.kdf` + `cryptography`, K5 CP1 section 13.1).
    let blob = id_ed25519_enc();
    let outer = crate::ssh::parser::parse_outer(&blob).unwrap();
    let seed = unwrap_seed(
        PASSPHRASE,
        outer.salt,
        outer.rounds,
        outer.private,
        &outer.public,
    )
    .unwrap();
    assert_eq!(public_of(&seed), id_ed25519_public());
    assert!(unwrap_seed(
        b"hunter43",
        outer.salt,
        outer.rounds,
        outer.private,
        &outer.public
    )
    .is_err());
}
