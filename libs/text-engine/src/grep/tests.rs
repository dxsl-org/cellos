use super::parse::parse;
use super::{execute, push_input, LoadedInput};
use crate::records::MAX_INPUT_BYTES;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// Every case here uses only inline operands, so the `-f` reader must never run.
fn no_pattern_files(path: &str) -> Result<String, String> {
    unreachable!("unexpected pattern-file read of {path}")
}

fn cfg(args: &[&str]) -> super::Config {
    let owned: Vec<String> = args.iter().map(|arg| arg.to_string()).collect();
    parse(&owned, &mut no_pattern_files).expect("config should parse")
}

fn input(label: Option<&str>, text: &str) -> LoadedInput {
    LoadedInput {
        label: label.map(str::to_owned),
        text: text.to_owned(),
    }
}

#[test]
fn grep_prefixes_multiple_files() {
    let outcome = execute(
        &cfg(&["-n", "foo", "a", "b"]),
        &[input(Some("a"), "foo\n"), input(Some("b"), "bar\nfoo\n")],
    )
    .unwrap();
    assert_eq!(outcome.status.exit_code(), 0);
    assert_eq!(outcome.output, "a:1:foo\nb:2:foo\n");
}

#[test]
fn grep_quiet_suppresses_output() {
    let outcome = execute(&cfg(&["-q", "foo"]), &[input(None, "foo\nbar\n")]).unwrap();
    assert_eq!(outcome.status.exit_code(), 0);
    assert!(outcome.output.is_empty());
}

#[test]
fn grep_exact_line_uses_ascii_matching() {
    let outcome = execute(
        &cfg(&["-Fxi", "alpha"]),
        &[input(None, "ALPHA\nalphabeta\n")],
    )
    .unwrap();
    assert_eq!(outcome.output, "ALPHA\n");
}

#[test]
fn grep_regex_and_invert_compose() {
    let outcome = execute(
        &cfg(&["-Evn", "^[A-Z]+$"]),
        &[input(None, "ABC\nabc\nXYZ\n")],
    )
    .unwrap();
    assert_eq!(outcome.output, "2:abc\n");
}

#[test]
fn grep_counts_per_input_and_reports_no_match() {
    let outcome = execute(&cfg(&["-c", "zzz"]), &[input(Some("a"), "foo\nbar\n")]).unwrap();
    assert_eq!(outcome.status.exit_code(), 1);
    assert_eq!(outcome.output, "a:0\n");
}

#[test]
fn grep_rejects_unknown_flags() {
    let err = parse(
        &[String::from("-z"), String::from("foo")],
        &mut no_pattern_files,
    )
    .unwrap_err();
    assert_eq!(err.message(), "unknown flag '-z'");
}

#[test]
fn grep_reports_invalid_pattern_as_error() {
    let err = execute(&cfg(&["-E", "a{2,1}"]), &[input(None, "aa\n")]).unwrap_err();
    assert!(!err.is_empty(), "compile failure must carry a diagnostic");
}

#[test]
fn aggregate_input_budget_rejects_the_first_excess_byte() {
    let mut inputs: Vec<LoadedInput> = Vec::new();
    let mut total = 0;
    push_input(
        &mut inputs,
        &mut total,
        None,
        String::from_utf8(vec![b'a'; MAX_INPUT_BYTES]).unwrap(),
    )
    .unwrap();
    assert!(push_input(&mut inputs, &mut total, None, String::from("b")).is_err());
    assert_eq!(inputs.len(), 1);
    assert_eq!(total, MAX_INPUT_BYTES);
}
