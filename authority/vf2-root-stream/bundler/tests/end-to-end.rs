mod support;

use std::fs;
use support::Fixture;
use vf2_root_stream_bundler::{bundler, verifier};

#[test]
fn deterministic_bundle_is_independently_verified() {
    let first = Fixture::new();
    bundler::run(first.bundler_args()).unwrap();
    let report = verifier::run(first.verifier_args()).unwrap();
    assert!(report.starts_with("evidence=SOFTWARE_HARNESS\nlane=DEV_REFERENCE\n"));
    assert!(
        report.contains("production_evidence=false\nphysical_evidence=false\nstatus=verified\n")
    );
    for name in ["opensbi", "dtb", "cellos", "vifs"] {
        assert!(report.contains(&format!("component={name} length=")));
    }

    let second = Fixture::new();
    bundler::run(second.bundler_args()).unwrap();
    assert_eq!(
        fs::read(&first.transcript).unwrap(),
        fs::read(&second.transcript).unwrap()
    );
    assert_eq!(
        fs::read(&first.summary).unwrap(),
        fs::read(&second.summary).unwrap()
    );
    let summary = fs::read_to_string(&first.summary).unwrap();
    assert!(summary.contains("evidence=SOFTWARE_HARNESS\nlane=DEV_REFERENCE\n"));
    assert!(!summary.contains(&"07".repeat(32)));
}

#[test]
fn altered_transcript_produces_no_success_report() {
    let fixture = Fixture::new();
    bundler::run(fixture.bundler_args()).unwrap();
    let mut transcript = fs::read(&fixture.transcript).unwrap();
    transcript[20] ^= 1;
    fs::write(&fixture.transcript, transcript).unwrap();
    assert!(verifier::run(fixture.verifier_args()).is_err());
}
