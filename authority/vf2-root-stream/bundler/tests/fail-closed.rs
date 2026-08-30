mod support;

use std::fs;
use support::Fixture;
use vf2_root_stream_bundler::{bundler, verifier};

#[test]
fn malformed_seed_and_public_key_are_rejected() {
    let bad_seed = Fixture::new();
    fs::write(bad_seed.root.join("seed.bin"), [7u8; 31]).unwrap();
    let error = bundler::run(bad_seed.bundler_args()).unwrap_err();
    assert!(error.contains("exactly 32 raw bytes"));
    assert!(!bad_seed.transcript.exists());
    assert!(!bad_seed.summary.exists());

    let bad_key = Fixture::new();
    bundler::run(bad_key.bundler_args()).unwrap();
    fs::write(&bad_key.key, [9u8; 31]).unwrap();
    let error = verifier::run(bad_key.verifier_args()).unwrap_err();
    assert!(error.contains("exactly 32 raw bytes"));
}

#[test]
fn oversized_inputs_fail_before_protocol_allocation() {
    let oversized_seed = Fixture::new();
    std::fs::File::options()
        .write(true)
        .open(oversized_seed.root.join("seed.bin"))
        .unwrap()
        .set_len(33)
        .unwrap();
    assert!(bundler::run(oversized_seed.bundler_args())
        .unwrap_err()
        .contains("32-byte limit"));

    let oversized_component = Fixture::new();
    std::fs::File::options()
        .write(true)
        .open(oversized_component.root.join("opensbi.bin"))
        .unwrap()
        .set_len(65_537)
        .unwrap();
    assert!(bundler::run(oversized_component.bundler_args())
        .unwrap_err()
        .contains("65536-byte limit"));

    let oversized_transcript = Fixture::new();
    bundler::run(oversized_transcript.bundler_args()).unwrap();
    std::fs::OpenOptions::new()
        .write(true)
        .open(&oversized_transcript.transcript)
        .unwrap()
        .set_len(4 * 1029 + 2)
        .unwrap();
    assert!(verifier::run(oversized_transcript.verifier_args())
        .unwrap_err()
        .contains("4117-byte limit"));
}

#[test]
fn output_collision_fails_without_replacing_or_leaving_an_output() {
    let fixture = Fixture::new();
    fs::write(&fixture.summary, b"owned-by-caller").unwrap();
    let error = bundler::run(fixture.bundler_args()).unwrap_err();
    assert!(error.contains("cannot create summary output"));
    assert_eq!(fs::read(&fixture.summary).unwrap(), b"owned-by-caller");
    assert!(!fixture.transcript.exists());
}

#[test]
fn all_zero_seed_and_key_are_rejected() {
    let zero_seed = Fixture::new();
    fs::write(zero_seed.root.join("seed.bin"), [0u8; 32]).unwrap();
    assert!(bundler::run(zero_seed.bundler_args())
        .unwrap_err()
        .contains("nonzero 32-byte seed"));

    let zero_key = Fixture::new();
    bundler::run(zero_key.bundler_args()).unwrap();
    fs::write(&zero_key.key, [0u8; 32]).unwrap();
    assert!(verifier::run(zero_key.verifier_args())
        .unwrap_err()
        .contains("nonzero 32-byte key"));
}

#[test]
fn staging_overlap_with_immutable_ranges_is_rejected() {
    let fixture = Fixture::new();
    let mut args = fixture.bundler_args();
    replace(&mut args, "--loader-range-base", "0x88000000");
    replace(&mut args, "--loader-range-end", "0x88001000");
    assert!(bundler::run(args).is_err());
    assert!(!fixture.transcript.exists());
    assert!(!fixture.summary.exists());
}

#[test]
fn immutable_windows_fail_before_any_bundle_input_is_read() {
    let fixture = Fixture::new();
    let mut args = fixture.bundler_args();
    replace(&mut args, "--opensbi-load-address", "0x88000000");
    replace(&mut args, "--entry-address", "0x88000000");
    replace(&mut args, "--opensbi-max-load-end", "0x88010000");
    fs::remove_file(fixture.root.join("opensbi.bin")).unwrap();
    fs::remove_file(fixture.root.join("seed.bin")).unwrap();
    let error = bundler::run(args).unwrap_err();
    assert!(error.contains("RangeOverlap"), "{error}");
    assert!(!fixture.transcript.exists());
    assert!(!fixture.summary.exists());
}

fn replace(args: &mut [std::ffi::OsString], name: &str, value: &str) {
    let index = args.iter().position(|item| item == name).unwrap();
    args[index + 1] = value.into();
}
