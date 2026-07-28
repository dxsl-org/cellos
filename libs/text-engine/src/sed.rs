mod parser;
mod pattern;
mod replacement;

use alloc::string::String;
use alloc::vec::Vec;
use parser::{Command, SedScript};

use crate::records::{RecordError, RecordReader, MAX_RECORD_BYTES};

#[derive(Debug)]
pub enum SedError {
    Parse(&'static str),
    Record(RecordError),
    OutputTooLong,
}

impl SedError {
    pub fn message(&self) -> &'static str {
        match self {
            Self::Parse(message) => message,
            Self::Record(RecordError::InputTooLarge) => "input exceeds configured limit",
            Self::Record(RecordError::RecordTooLong) => "record exceeds configured limit",
            Self::Record(RecordError::TooManyRecords) => "record count exceeds configured limit",
            Self::OutputTooLong => "output record exceeds configured limit",
        }
    }
}

struct LineOutcome {
    line: String,
    deleted: bool,
    explicit_print: bool,
}

pub fn execute(script: &str, suppress_default: bool, input: &str) -> Result<Vec<String>, SedError> {
    let script = parser::parse(script)?;
    let records = RecordReader::new(input)
        .map_err(SedError::Record)?
        .collect()
        .map_err(SedError::Record)?;
    let mut output = Vec::new();
    for (index, record) in records.iter().enumerate() {
        let outcome = apply_record(&script, record, index + 1)?;
        if outcome.deleted {
            continue;
        }
        if outcome.explicit_print {
            output.push(outcome.line.clone());
        }
        if !suppress_default {
            output.push(outcome.line);
        }
    }
    Ok(output)
}

fn apply_record(
    script: &SedScript,
    line: &str,
    line_number: usize,
) -> Result<LineOutcome, SedError> {
    match &script.command {
        Command::Substitute {
            pattern,
            replacement,
            global,
            print,
        } => {
            let (line, changed) = pattern.replace(line, replacement, *global)?;
            Ok(LineOutcome {
                line,
                deleted: false,
                explicit_print: *print && changed,
            })
        }
        Command::Delete(address) => Ok(LineOutcome {
            line: String::from(line),
            deleted: address.matches(line, line_number),
            explicit_print: false,
        }),
        Command::Print(address) => Ok(LineOutcome {
            line: String::from(line),
            deleted: false,
            explicit_print: address.matches(line, line_number),
        }),
    }
}

pub(super) fn validate_output_len(line: &str) -> Result<(), SedError> {
    if line.len() > MAX_RECORD_BYTES {
        Err(SedError::OutputTooLong)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::execute;
    use alloc::vec::Vec;

    fn run(script: &str, suppress: bool, input: &str) -> Vec<String> {
        execute(script, suppress, input).expect("script should succeed")
    }

    #[test]
    fn substitutes_and_expands_match() {
        let output = run(r"s|foo([0-9]+)|<&>|gp", false, "foo12\nbar");
        assert_eq!(output, ["<foo12>", "<foo12>", "bar"]);
    }

    #[test]
    fn supports_address_print_and_delete() {
        let printed = run(r"/^ERR/p", true, "OK\nERR one\nERR two");
        assert_eq!(printed, ["ERR one", "ERR two"]);
        let deleted = run(r"/^ERR/d", false, "OK\nERR one\nWARN");
        assert_eq!(deleted, ["OK", "WARN"]);
    }

    #[test]
    fn supports_numeric_print() {
        let output = run("2p", true, "one\ntwo\nthree");
        assert_eq!(output, ["two"]);
    }
}
