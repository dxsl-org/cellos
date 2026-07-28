//! Shell adapter for the pure grep core in `libs/text-engine`.
//!
//! Loads operands (stdin, VFS files, recursive walks), then hands them to
//! `text_engine::grep` and prints the resulting output block.

extern crate alloc;

mod io;

use crate::executor::{shell_print, shell_println};
use alloc::string::String;
use text_engine::args::UtilityStatus;
use text_engine::grep::{self, ParseError};

pub fn run(args: &[String]) -> i32 {
    // `-f PATTERNFILE` is the only VFS read the parser needs; injecting it keeps
    // option parsing pure and host-testable.
    let mut read_pattern_file =
        |path: &str| io::read_text_file(path).map_err(|err| String::from(err.message()));
    let cfg = match grep::parse(args, &mut read_pattern_file) {
        Ok(cfg) => cfg,
        Err(err) => return parse_error(err),
    };
    let inputs = match io::load_inputs(&cfg) {
        Ok(inputs) => inputs,
        Err(message) => return print_error(&message),
    };
    let outcome = match grep::execute(&cfg, &inputs) {
        Ok(outcome) => outcome,
        Err(message) => return print_error(&message),
    };
    shell_print(&outcome.output);
    outcome.status.exit_code()
}

fn parse_error(err: ParseError) -> i32 {
    print_error(err.message())
}

fn print_error(message: &str) -> i32 {
    shell_print("grep: ");
    shell_println(message);
    UtilityStatus::Error.exit_code()
}
