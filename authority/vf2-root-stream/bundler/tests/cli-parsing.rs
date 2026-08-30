mod support;

use std::ffi::OsString;
use support::Fixture;
use vf2_root_stream_bundler::bundler_args::BundlerArgs;
use vf2_root_stream_bundler::verifier_args::VerifierArgs;

#[test]
fn missing_unknown_and_duplicate_arguments_are_rejected() {
    let missing = BundlerArgs::parse([OsString::from("bundler")]).unwrap_err();
    assert!(missing.contains("missing required argument --opensbi"));

    let fixture = Fixture::new();
    let mut unknown = fixture.bundler_args();
    unknown.extend(["--surprise".into(), "1".into()]);
    assert!(BundlerArgs::parse(unknown)
        .unwrap_err()
        .contains("unknown argument --surprise"));

    let mut duplicate = fixture.bundler_args();
    duplicate.extend(["--boot-epoch".into(), "42".into()]);
    assert!(BundlerArgs::parse(duplicate)
        .unwrap_err()
        .contains("duplicate argument --boot-epoch"));
}

#[test]
fn malformed_hex_zero_freshness_and_bad_numbers_are_rejected() {
    let fixture = Fixture::new();
    let mut malformed = fixture.verifier_args();
    replace(&mut malformed, "--device-id", &"gg".repeat(32));
    assert!(VerifierArgs::parse(malformed)
        .unwrap_err()
        .contains("exactly 64 hexadecimal characters"));

    let mut zero = fixture.verifier_args();
    replace(&mut zero, "--request-id", "0");
    assert!(VerifierArgs::parse(zero)
        .unwrap_err()
        .contains("--request-id must be nonzero"));

    let mut bad_number = fixture.verifier_args();
    replace(&mut bad_number, "--staging-base", "-1");
    assert!(VerifierArgs::parse(bad_number)
        .unwrap_err()
        .contains("decimal integer or 0x-prefixed"));
}

fn replace(args: &mut [OsString], name: &str, value: &str) {
    let index = args.iter().position(|item| item == name).unwrap();
    args[index + 1] = value.into();
}
