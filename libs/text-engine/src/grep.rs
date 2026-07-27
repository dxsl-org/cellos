//! Pure grep core: option parsing, pattern compilation, and record selection.
//!
//! Nothing here touches stdin, the VFS, or the terminal — the caller loads the
//! text (see [`LoadedInput`]) and prints [`Outcome::output`].  Pattern files
//! (`-f`) are read through a caller-supplied reader so the parser stays pure.

pub mod parse;

#[cfg(test)]
mod tests;

pub use parse::{parse, Config, ParseError, PatternFileReader};

use crate::args::UtilityStatus;
use crate::matcher::{CompiledPattern, PatternKind};
use crate::records::{RecordError, RecordReader, MAX_INPUT_BYTES};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// One named text operand: `label` is the display prefix (`None` for stdin).
pub struct LoadedInput {
    pub label: Option<String>,
    pub text: String,
}

#[derive(Debug)]
pub struct Outcome {
    pub output: String,
    pub status: UtilityStatus,
}

struct MatcherSpec {
    compiled: CompiledPattern,
    case_insensitive: bool,
    exact_line: bool,
}

/// Append `text` to `inputs` while enforcing the aggregate input budget.
///
/// # Errors
/// Returns a diagnostic once the running total would exceed
/// [`MAX_INPUT_BYTES`]; `inputs` is left untouched in that case.
pub fn push_input(
    inputs: &mut Vec<LoadedInput>,
    total_bytes: &mut usize,
    label: Option<String>,
    text: String,
) -> Result<(), String> {
    *total_bytes = total_bytes
        .checked_add(text.len())
        .filter(|total| *total <= MAX_INPUT_BYTES)
        .ok_or_else(|| String::from("aggregate input exceeds 65536-byte limit"))?;
    inputs.push(LoadedInput { label, text });
    Ok(())
}

/// Select records from `inputs` according to `cfg`.
///
/// # Errors
/// Returns a diagnostic when a pattern fails to compile within the matcher
/// limits or an input violates a record limit.
pub fn execute(cfg: &Config, inputs: &[LoadedInput]) -> Result<Outcome, String> {
    let matchers = compile_matchers(cfg)?;
    let mut output = String::new();
    let mut selected_any = false;
    for input in inputs {
        let mut selected = 0usize;
        let mut records = RecordReader::new(&input.text).map_err(record_error)?;
        let mut line_no = 0usize;
        while let Some(line) = records.next_record().map_err(record_error)? {
            line_no += 1;
            if matches_any(&matchers, line) ^ cfg.invert {
                selected += 1;
                selected_any = true;
                if cfg.quiet {
                    return Ok(Outcome {
                        output,
                        status: UtilityStatus::Selected,
                    });
                }
                if !cfg.count_only {
                    push_prefix(
                        &mut output,
                        input.label.as_deref(),
                        cfg.line_numbers,
                        line_no,
                    );
                    output.push_str(line);
                    output.push('\n');
                }
            }
        }
        if cfg.count_only && !cfg.quiet {
            push_prefix(&mut output, input.label.as_deref(), false, 0);
            output.push_str(&format!("{selected}\n"));
        }
    }
    Ok(Outcome {
        output,
        status: if selected_any {
            UtilityStatus::Selected
        } else {
            UtilityStatus::NotSelected
        },
    })
}

fn compile_matchers(cfg: &Config) -> Result<Vec<MatcherSpec>, String> {
    cfg.patterns
        .iter()
        .map(|pattern| {
            let raw = if cfg.exact_line && cfg.kind == PatternKind::ERELite {
                format!("^(?:{pattern})$")
            } else {
                pattern.clone()
            };
            CompiledPattern::compile(cfg.kind, &raw, cfg.case_insensitive)
                .map(|compiled| MatcherSpec {
                    compiled,
                    case_insensitive: cfg.case_insensitive,
                    exact_line: cfg.exact_line,
                })
                .map_err(|err| String::from(err.message()))
        })
        .collect()
}

fn matches_any(matchers: &[MatcherSpec], line: &str) -> bool {
    matchers.iter().any(|matcher| {
        if matcher.exact_line {
            if let Some(literal) = matcher.compiled.literal() {
                return exact_equals(line, literal, matcher.case_insensitive);
            }
        }
        matcher.compiled.is_match(line)
    })
}

fn exact_equals(line: &str, literal: &str, case_insensitive: bool) -> bool {
    if !case_insensitive {
        return line == literal;
    }
    line.len() == literal.len()
        && line
            .as_bytes()
            .iter()
            .zip(literal.as_bytes())
            .all(|(lhs, rhs)| lhs.eq_ignore_ascii_case(rhs))
}

fn push_prefix(output: &mut String, label: Option<&str>, line_numbers: bool, line_no: usize) {
    if let Some(label) = label {
        output.push_str(label);
        output.push(':');
    }
    if line_numbers {
        output.push_str(&format!("{line_no}:"));
    }
}

pub fn record_error(err: RecordError) -> String {
    String::from(match err {
        RecordError::InputTooLarge => "input exceeds configured limit",
        RecordError::RecordTooLong => "record exceeds configured limit",
        RecordError::TooManyRecords => "record count exceeds configured limit",
    })
}
