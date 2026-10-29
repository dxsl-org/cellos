//! Streaming plain-text rendering of `cellbox.search.line` rows (proposal §3.6; D74-3,
//! D76-11, D76-12).
//!
//! One row renders as `ESCAPED_PATH (':' | '-') LINE (':' | '-') ESCAPED_LINE '\n'`, `:`
//! for a match and `-` for context, `<stdin>` for a Null path. The path is the record's
//! absolute canonical path. Escaping keeps printable valid UTF-8 and writes everything
//! else as ASCII escapes, so the output is ASCII plus valid UTF-8 and never carries a
//! terminal control byte (in particular no ESC).

use alloc::format;
use alloc::vec::Vec;
use text_engine::pipeline::{PipelineError, Value};

const SCHEMA: &str = "cellbox.search.line";
const HEX: &[u8; 16] = b"0123456789ABCDEF";

/// Append `path` escaped for display: [`escape_line`] plus `:` → `\x3A` and `-` → `\x2D`,
/// so the `PATH:LINE:` / `PATH-LINE-` separators stay unambiguous.
pub fn escape_path(path: &[u8], out: &mut Vec<u8>) {
    escape(path, true, out);
}

/// Append `line` escaped for display: `\` → `\\`; LF, CR, tab, NUL → `\n`, `\r`, `\t`,
/// `\0`; every byte of other controls (C0, DEL, C1), of the bidi/format controls U+061C,
/// U+200E, U+200F, U+202A–U+202E, U+2066–U+2069 and of invalid UTF-8 → `\xHH`
/// (uppercase). Printable valid UTF-8 is kept.
pub fn escape_line(line: &[u8], out: &mut Vec<u8>) {
    escape(line, false, out);
}

fn escape(bytes: &[u8], path: bool, out: &mut Vec<u8>) {
    for chunk in bytes.utf8_chunks() {
        let text = chunk.valid();
        // Start of the pending run of characters copied unchanged.
        let mut run = 0;
        for (at, c) in text.char_indices() {
            let kept = match c {
                '\\' | '\n' | '\r' | '\t' | '\0' => false,
                ':' | '-' => !path,
                c => !hidden(c),
            };
            if kept {
                continue;
            }
            out.extend_from_slice(&text.as_bytes()[run..at]);
            run = at + c.len_utf8();
            match c {
                '\\' => out.extend_from_slice(b"\\\\"),
                '\n' => out.extend_from_slice(b"\\n"),
                '\r' => out.extend_from_slice(b"\\r"),
                '\t' => out.extend_from_slice(b"\\t"),
                '\0' => out.extend_from_slice(b"\\0"),
                _ => text.as_bytes()[at..run].iter().for_each(|&b| hex(b, out)),
            }
        }
        out.extend_from_slice(&text.as_bytes()[run..]);
        chunk.invalid().iter().for_each(|&b| hex(b, out));
    }
}

/// Characters never shown raw: Cc controls (C0, DEL, C1) and the D76-11 bidi/format
/// controls, which are not printable and can reorder the displayed text.
fn hidden(c: char) -> bool {
    c.is_control()
        || matches!(c, '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

fn hex(byte: u8, out: &mut Vec<u8>) {
    out.extend_from_slice(&[b'\\', b'x', HEX[usize::from(byte >> 4)], HEX[usize::from(byte & 0xF)]]);
}

fn decimal(mut n: u64, out: &mut Vec<u8>) {
    let mut digits = [0u8; 20];
    let mut start = digits.len();
    loop {
        start -= 1;
        digits[start] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    out.extend_from_slice(&digits[start..]);
}

fn not_search(field: &str) -> PipelineError {
    PipelineError::Sequence(format!(
        "text renderer: row is not a {} record (field '{}' missing or mistyped)",
        SCHEMA, field
    ))
}

fn field<'a>(row: &'a Value, name: &str) -> Result<&'a Value, PipelineError> {
    row.get_field(name).ok_or_else(|| not_search(name))
}

/// Append the text form of one `cellbox.search.line` row to `out`. A row that is not such
/// a record is an error and leaves `out` unchanged.
pub fn render_row(row: &Value, out: &mut Vec<u8>) -> Result<(), PipelineError> {
    match field(row, "schema")? {
        Value::String(schema) if schema == SCHEMA => {}
        _ => return Err(not_search("schema")),
    }
    if *field(row, "schema_version")? != Value::UInt(1) {
        return Err(not_search("schema_version"));
    }
    let separator = match field(row, "kind")? {
        Value::String(kind) if kind == "match" => b':',
        Value::String(kind) if kind == "context" => b'-',
        _ => return Err(not_search("kind")),
    };
    let path = match field(row, "path")? {
        Value::Path(path) => Some(path),
        Value::Null => None,
        _ => return Err(not_search("path")),
    };
    let Value::UInt(line_number) = *field(row, "line_number")? else {
        return Err(not_search("line_number"));
    };
    let Value::Bytes(line) = field(row, "line_bytes")? else {
        return Err(not_search("line_bytes"));
    };

    match path {
        Some(path) => escape_path(path, out),
        None => out.extend_from_slice(b"<stdin>"),
    }
    out.push(separator);
    decimal(line_number, out);
    out.push(separator);
    escape_line(line, out);
    out.push(b'\n');
    Ok(())
}
