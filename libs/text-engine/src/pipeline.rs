// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Typed command data model, bounded pipeline transforms, and serialization for Cellbox.

extern crate alloc;
use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};
use core::cmp::Ordering;
use core::fmt;

/// Maximum nesting depth for lists and records.
pub const MAX_DEPTH: usize = 16;
/// Maximum length of a record field name in UTF-8 bytes.
pub const MAX_FIELD_NAME_LEN: usize = 128;
/// Maximum number of fields in a single record.
pub const MAX_RECORD_FIELDS: usize = 128;
/// Maximum serialized size of a single record line in bytes.
pub const MAX_RECORD_LINE_BYTES: usize = 65536;
/// Maximum records allowed in a global sort buffer.
pub const MAX_SORT_RECORDS: usize = 4096;
/// Maximum aggregate payload bytes allowed in a global sort buffer (4 MiB).
pub const MAX_SORT_PAYLOAD_BYTES: usize = 4 * 1024 * 1024;
/// Maximum safe integer representable without precision loss in standard JSON (2^53 - 1).
pub const MAX_SAFE_JSON_INT: i64 = 9_007_199_254_740_991;
pub const MIN_SAFE_JSON_INT: i64 = -9_007_199_254_740_991;

/// Pipeline and data manipulation error types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PipelineError {
    MaxDepthExceeded,
    FieldNameTooLong,
    TooManyFields,
    RecordTooLarge,
    SortLimitExceeded,
    NonFiniteFloat,
    TypeMismatch,
    FieldNotFound(String),
    DuplicateField(String),
    PrecisionLoss(String),
    ParseError(String),
    UnsupportedType(String),
    /// Invalid stage arguments (usage error, no effects).
    Usage(String),
    /// Statically invalid stage sequence.
    Sequence(String),
    /// Producer-side I/O failure reported by a native row source.
    Source(String),
}

impl fmt::Display for PipelineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MaxDepthExceeded => write!(f, "maximum nesting depth (16) exceeded"),
            Self::FieldNameTooLong => write!(f, "field name exceeds 128 bytes"),
            Self::TooManyFields => write!(f, "record exceeds 128 fields"),
            Self::RecordTooLarge => write!(f, "record exceeds 65536 bytes"),
            Self::SortLimitExceeded => write!(f, "sort buffer exceeded 4096 records or 4 MiB"),
            Self::NonFiniteFloat => write!(f, "non-finite float (NaN or Inf) is not allowed"),
            Self::TypeMismatch => write!(f, "type mismatch during comparison"),
            Self::FieldNotFound(k) => write!(f, "field '{}' not found", k),
            Self::DuplicateField(k) => write!(f, "duplicate field '{}'", k),
            Self::PrecisionLoss(msg) => write!(f, "precision loss: {}", msg),
            Self::ParseError(msg) => write!(f, "JSON parse error: {}", msg),
            Self::UnsupportedType(msg) => write!(f, "unsupported type conversion: {}", msg),
            Self::Usage(msg) => write!(f, "{}", msg),
            Self::Sequence(msg) => write!(f, "{}", msg),
            Self::Source(msg) => write!(f, "{}", msg),
        }
    }
}

/// Tagged native Value enum for structured pipelines.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    UInt(u64),
    Float(f64),
    String(String),
    Bytes(Vec<u8>),
    Path(Vec<u8>),
    Timestamp(i64),
    Duration(i64),
    List(Vec<Value>),
    Record(Vec<(String, Value)>),
}

impl Value {
    /// Calculate the approximate memory payload in bytes.
    pub fn payload_bytes(&self) -> usize {
        match self {
            Self::Null => 0,
            Self::Bool(_) => 1,
            Self::Int(_) | Self::UInt(_) | Self::Float(_) | Self::Timestamp(_) | Self::Duration(_) => 8,
            Self::String(s) => s.len(),
            Self::Bytes(b) | Self::Path(b) => b.len(),
            Self::List(l) => l.iter().map(|v| v.payload_bytes()).sum(),
            Self::Record(r) => r.iter().map(|(k, v)| k.len() + v.payload_bytes()).sum(),
        }
    }

    /// Validate structural bounds (depth, field name length, field count, duplicate fields).
    pub fn validate(&self) -> Result<(), PipelineError> {
        self.validate_internal(0)
    }

    fn validate_internal(&self, depth: usize) -> Result<(), PipelineError> {
        if depth > MAX_DEPTH {
            return Err(PipelineError::MaxDepthExceeded);
        }

        match self {
            Self::Float(f) => {
                if !f.is_finite() {
                    return Err(PipelineError::NonFiniteFloat);
                }
            }
            Self::List(list) => {
                for item in list {
                    item.validate_internal(depth + 1)?;
                }
            }
            Self::Record(fields) => {
                if fields.len() > MAX_RECORD_FIELDS {
                    return Err(PipelineError::TooManyFields);
                }
                for i in 0..fields.len() {
                    let (name, val) = &fields[i];
                    if name.len() > MAX_FIELD_NAME_LEN {
                        return Err(PipelineError::FieldNameTooLong);
                    }
                    for (other_name, _) in &fields[i + 1..] {
                        if name == other_name {
                            return Err(PipelineError::DuplicateField(name.clone()));
                        }
                    }
                    val.validate_internal(depth + 1)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Extract a field reference from a Record value.
    pub fn get_field(&self, key: &str) -> Option<&Value> {
        if let Self::Record(fields) = self {
            for (k, v) in fields {
                if k == key {
                    return Some(v);
                }
            }
        }
        None
    }
}

/// Compare two Values under the accepted typing rules.
pub fn compare_values(a: &Value, b: &Value) -> Result<Ordering, PipelineError> {
    match (a, b) {
        (Value::Null, Value::Null) => Ok(Ordering::Equal),
        (Value::Null, _) => Ok(Ordering::Less),
        (_, Value::Null) => Ok(Ordering::Greater),

        (Value::Bool(x), Value::Bool(y)) => Ok(x.cmp(y)),

        (Value::Int(x), Value::Int(y)) => Ok(x.cmp(y)),
        (Value::UInt(x), Value::UInt(y)) => Ok(x.cmp(y)),
        (Value::Int(x), Value::UInt(y)) => {
            if *x < 0 {
                Ok(Ordering::Less)
            } else {
                Ok((*x as u64).cmp(y))
            }
        }
        (Value::UInt(x), Value::Int(y)) => {
            if *y < 0 {
                Ok(Ordering::Greater)
            } else {
                Ok(x.cmp(&(*y as u64)))
            }
        }

        (Value::Float(x), Value::Float(y)) => {
            if !x.is_finite() || !y.is_finite() {
                return Err(PipelineError::NonFiniteFloat);
            }
            x.partial_cmp(y).ok_or(PipelineError::NonFiniteFloat)
        }

        (Value::String(x), Value::String(y)) => Ok(x.cmp(y)),
        (Value::Bytes(x), Value::Bytes(y)) => Ok(x.cmp(y)),
        (Value::Path(x), Value::Path(y)) => Ok(x.cmp(y)),
        (Value::Timestamp(x), Value::Timestamp(y)) => Ok(x.cmp(y)),
        (Value::Duration(x), Value::Duration(y)) => Ok(x.cmp(y)),

        _ => Err(PipelineError::TypeMismatch),
    }
}

/// Comparison operators for `where` filtering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareOp {
    Equal,
    NotEqual,
    LessThan,
    LessOrEqual,
    GreaterThan,
    GreaterOrEqual,
}

impl CompareOp {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "==" => Some(Self::Equal),
            "!=" => Some(Self::NotEqual),
            "<" => Some(Self::LessThan),
            "<=" => Some(Self::LessOrEqual),
            ">" => Some(Self::GreaterThan),
            ">=" => Some(Self::GreaterOrEqual),
            _ => None,
        }
    }

    pub fn matches(&self, ord: Ordering) -> bool {
        match self {
            Self::Equal => ord == Ordering::Equal,
            Self::NotEqual => ord != Ordering::Equal,
            Self::LessThan => ord == Ordering::Less,
            Self::LessOrEqual => ord == Ordering::Less || ord == Ordering::Equal,
            Self::GreaterThan => ord == Ordering::Greater,
            Self::GreaterOrEqual => ord == Ordering::Greater || ord == Ordering::Equal,
        }
    }
}

// ─── Pipeline Transforms ─────────────────────────────────────────────────────

/// Project specified fields from a Record in the exact order given.
pub fn select_fields(record: &Value, fields: &[&str]) -> Result<Value, PipelineError> {
    if let Value::Record(orig) = record {
        let mut projected = Vec::with_capacity(fields.len());
        for &f in fields {
            let mut found = None;
            for (k, v) in orig {
                if k == f {
                    found = Some(v.clone());
                    break;
                }
            }
            match found {
                Some(val) => projected.push((f.to_string(), val)),
                None => return Err(PipelineError::FieldNotFound(f.to_string())),
            }
        }
        Ok(Value::Record(projected))
    } else {
        Err(PipelineError::TypeMismatch)
    }
}

/// Evaluate `where FIELD OP LITERAL` filter on a record.
pub fn filter_record(
    record: &Value,
    field: &str,
    op: CompareOp,
    literal: &Value,
) -> Result<bool, PipelineError> {
    let field_val = record
        .get_field(field)
        .ok_or_else(|| PipelineError::FieldNotFound(field.to_string()))?;

    // Null equality is defined; every ordering comparison involving Null is false.
    if matches!(field_val, Value::Null) || matches!(literal, Value::Null) {
        let both_null = matches!(field_val, Value::Null) && matches!(literal, Value::Null);
        return Ok(match op {
            CompareOp::Equal => both_null,
            CompareOp::NotEqual => !both_null,
            _ => false,
        });
    }

    let ord = compare_values(field_val, literal)?;
    if matches!(field_val, Value::Bool(_))
        && !matches!(op, CompareOp::Equal | CompareOp::NotEqual)
    {
        return Err(PipelineError::UnsupportedType(
            "bool supports only == and !=".to_string(),
        ));
    }
    Ok(op.matches(ord))
}

/// Sort records stably by field value with safety and memory bounds.
pub fn sort_records_by(
    records: &mut [Value],
    field: &str,
    reverse: bool,
) -> Result<(), PipelineError> {
    if records.len() > MAX_SORT_RECORDS {
        return Err(PipelineError::SortLimitExceeded);
    }
    let total_bytes: usize = records.iter().map(|r| r.payload_bytes()).sum();
    if total_bytes > MAX_SORT_PAYLOAD_BYTES {
        return Err(PipelineError::SortLimitExceeded);
    }

    // Verify all records possess the sorting field before sorting
    for r in records.iter() {
        if r.get_field(field).is_none() {
            return Err(PipelineError::FieldNotFound(field.to_string()));
        }
    }

    let mut err = None;
    records.sort_by(|a, b| {
        if err.is_some() {
            return Ordering::Equal;
        }
        let val_a = a.get_field(field).unwrap();
        let val_b = b.get_field(field).unwrap();
        match compare_values(val_a, val_b) {
            Ok(ord) => {
                if reverse {
                    ord.reverse()
                } else {
                    ord
                }
            }
            Err(e) => {
                err = Some(e);
                Ordering::Equal
            }
        }
    });

    if let Some(e) = err {
        return Err(e);
    }

    Ok(())
}

/// Truncate records to at most `n`.
pub fn take_records(records: &mut Vec<Value>, n: usize) {
    records.truncate(n);
}

// ─── JSON and Typed Serialization ────────────────────────────────────────────

/// Convert a Value to a JSON string (either plain or typed envelope).
pub fn value_to_json(val: &Value, typed: bool) -> Result<String, PipelineError> {
    let mut out = String::new();
    if typed {
        serialize_typed_line(val, &mut out)?;
    } else {
        serialize_untyped_node(val, &mut out)?;
    }
    if out.len() > MAX_RECORD_LINE_BYTES {
        return Err(PipelineError::RecordTooLarge);
    }
    Ok(out)
}

/// Length in bytes of `value_to_json(val, true)?` without building it (no allocation on
/// success), with the same errors: it shares the encoder with [`value_to_json`].
pub fn typed_json_len(val: &Value) -> Result<usize, PipelineError> {
    let mut len = JsonLen(0);
    serialize_typed_line(val, &mut len)?;
    if len.0 > MAX_RECORD_LINE_BYTES {
        return Err(PipelineError::RecordTooLarge);
    }
    Ok(len.0)
}

/// Infallible output of the JSON encoder: a `String`, or a [`JsonLen`] byte counter.
trait JsonOut {
    fn put(&mut self, s: &str);
    fn put_char(&mut self, c: char);
    fn put_fmt(&mut self, args: fmt::Arguments<'_>);
}

impl JsonOut for String {
    fn put(&mut self, s: &str) {
        self.push_str(s);
    }

    fn put_char(&mut self, c: char) {
        self.push(c);
    }

    fn put_fmt(&mut self, args: fmt::Arguments<'_>) {
        // A String sink only fails if a formatted value's impl fails; numbers never do.
        let _ = fmt::Write::write_fmt(self, args);
    }
}

/// Counts the bytes the encoder would write.
struct JsonLen(usize);

impl fmt::Write for JsonLen {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.0 = self.0.saturating_add(s.len());
        Ok(())
    }
}

impl JsonOut for JsonLen {
    fn put(&mut self, s: &str) {
        self.0 = self.0.saturating_add(s.len());
    }

    fn put_char(&mut self, c: char) {
        self.0 = self.0.saturating_add(c.len_utf8());
    }

    fn put_fmt(&mut self, args: fmt::Arguments<'_>) {
        // `write_str` above never fails.
        let _ = fmt::Write::write_fmt(self, args);
    }
}

fn serialize_typed_line<W: JsonOut>(val: &Value, out: &mut W) -> Result<(), PipelineError> {
    out.put("{\"cellbox\":1,\"value\":");
    serialize_typed_node(val, out)?;
    out.put_char('}');
    Ok(())
}

fn serialize_untyped_node<W: JsonOut>(val: &Value, out: &mut W) -> Result<(), PipelineError> {
    match val {
        Value::Null => out.put("null"),
        Value::Bool(b) => out.put(if *b { "true" } else { "false" }),
        Value::Int(n) => {
            if *n < MIN_SAFE_JSON_INT || *n > MAX_SAFE_JSON_INT {
                return Err(PipelineError::PrecisionLoss(format!(
                    "integer {} exceeds safe JSON range",
                    n
                )));
            }
            out.put_fmt(format_args!("{}", n));
        }
        Value::UInt(n) => {
            if *n > MAX_SAFE_JSON_INT as u64 {
                return Err(PipelineError::PrecisionLoss(format!(
                    "uint {} exceeds safe JSON range",
                    n
                )));
            }
            out.put_fmt(format_args!("{}", n));
        }
        Value::Float(f) => {
            if !f.is_finite() {
                return Err(PipelineError::NonFiniteFloat);
            }
            out.put_fmt(format_args!("{:?}", f));
        }
        Value::String(s) => write_json_escaped_string(s, out),
        Value::Bytes(_) | Value::Path(_) | Value::Timestamp(_) | Value::Duration(_) => {
            return Err(PipelineError::UnsupportedType(
                "Bytes/Path/Timestamp/Duration require --typed mode".to_string(),
            ));
        }
        Value::List(list) => {
            out.put_char('[');
            for (i, item) in list.iter().enumerate() {
                if i > 0 {
                    out.put_char(',');
                }
                serialize_untyped_node(item, out)?;
            }
            out.put_char(']');
        }
        Value::Record(fields) => {
            out.put_char('{');
            for (i, (k, v)) in fields.iter().enumerate() {
                if i > 0 {
                    out.put_char(',');
                }
                write_json_escaped_string(k, out);
                out.put_char(':');
                serialize_untyped_node(v, out)?;
            }
            out.put_char('}');
        }
    }
    Ok(())
}

fn serialize_typed_node<W: JsonOut>(val: &Value, out: &mut W) -> Result<(), PipelineError> {
    match val {
        Value::Null => out.put("[\"null\",null]"),
        Value::Bool(b) => out.put(if *b { "[\"bool\",true]" } else { "[\"bool\",false]" }),
        Value::Int(n) => out.put_fmt(format_args!("[\"int\",\"{}\"]", n)),
        Value::UInt(n) => out.put_fmt(format_args!("[\"uint\",\"{}\"]", n)),
        Value::Float(f) => {
            if !f.is_finite() {
                return Err(PipelineError::NonFiniteFloat);
            }
            out.put_fmt(format_args!("[\"float\",{:?}]", f));
        }
        Value::String(s) => {
            out.put("[\"string\",");
            write_json_escaped_string(s, out);
            out.put_char(']');
        }
        Value::Bytes(b) => {
            out.put("[\"bytes\",\"");
            write_hex(b, out);
            out.put("\"]");
        }
        Value::Path(p) => {
            out.put("[\"path\",\"");
            write_hex(p, out);
            out.put("\"]");
        }
        Value::Timestamp(t) => out.put_fmt(format_args!("[\"timestamp\",\"{}\"]", t)),
        Value::Duration(d) => out.put_fmt(format_args!("[\"duration\",\"{}\"]", d)),
        Value::List(list) => {
            out.put("[\"list\",[");
            for (i, item) in list.iter().enumerate() {
                if i > 0 {
                    out.put_char(',');
                }
                serialize_typed_node(item, out)?;
            }
            out.put("]]");
        }
        Value::Record(fields) => {
            out.put("[\"record\",[");
            for (i, (k, v)) in fields.iter().enumerate() {
                if i > 0 {
                    out.put_char(',');
                }
                out.put_char('[');
                write_json_escaped_string(k, out);
                out.put_char(',');
                serialize_typed_node(v, out)?;
                out.put_char(']');
            }
            out.put("]]");
        }
    }
    Ok(())
}

/// Lowercase hex, two digits per byte.
fn write_hex<W: JsonOut>(bytes: &[u8], out: &mut W) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for &byte in bytes {
        out.put_char(char::from(HEX[usize::from(byte >> 4)]));
        out.put_char(char::from(HEX[usize::from(byte & 0x0f)]));
    }
}

fn write_json_escaped_string<W: JsonOut>(s: &str, out: &mut W) {
    out.put_char('"');
    for c in s.chars() {
        match c {
            '"' => out.put("\\\""),
            '\\' => out.put("\\\\"),
            '\n' => out.put("\\n"),
            '\r' => out.put("\\r"),
            '\t' => out.put("\\t"),
            '\x08' => out.put("\\b"),
            '\x0C' => out.put("\\f"),
            c if c.is_control() => out.put_fmt(format_args!("\\u{:04x}", c as u32)),
            c => out.put_char(c),
        }
    }
    out.put_char('"');
}

/// Convert scalar value to plain text representation.
pub fn value_to_text(val: &Value, field: Option<&str>) -> Result<String, PipelineError> {
    match val {
        Value::String(s) => Ok(s.clone()),
        Value::Bytes(b) => match core::str::from_utf8(b) {
            Ok(s) => Ok(s.to_string()),
            Err(_) => Err(PipelineError::UnsupportedType("non-UTF-8 bytes".to_string())),
        },
        Value::Path(p) => match core::str::from_utf8(p) {
            Ok(s) => Ok(s.to_string()),
            Err(_) => Err(PipelineError::UnsupportedType("non-UTF-8 path".to_string())),
        },
        Value::Int(n) => Ok(n.to_string()),
        Value::UInt(n) => Ok(n.to_string()),
        Value::Float(f) => Ok(format!("{}", f)),
        Value::Bool(b) => Ok((if *b { "true" } else { "false" }).to_string()),
        Value::Null => Ok(String::new()),
        Value::Timestamp(t) => Ok(t.to_string()),
        Value::Duration(d) => Ok(d.to_string()),
        Value::Record(_) => {
            if let Some(target_field) = field {
                let f_val = val
                    .get_field(target_field)
                    .ok_or_else(|| PipelineError::FieldNotFound(target_field.to_string()))?;
                value_to_text(f_val, None)
            } else {
                Err(PipelineError::UnsupportedType(
                    "record requires --field FIELD for text conversion".to_string(),
                ))
            }
        }
        Value::List(_) => Err(PipelineError::UnsupportedType(
            "list cannot be converted to plain text".to_string(),
        )),
    }
}

// ─── Stage model ─────────────────────────────────────────────────────────────

/// Maximum number of stages in one pipeline.
pub const MAX_STAGES: usize = 16;
/// Native per-edge queue bound (records).
pub const MAX_EDGE_RECORDS: usize = 8;
/// Native per-edge resident payload bound (bytes).
pub const MAX_EDGE_PAYLOAD: usize = 256 * 1024;
/// Maximum size of one `from json` document in bytes (matches the sort payload bound).
pub const MAX_JSON_DOCUMENT_BYTES: usize = 4 * 1024 * 1024;

/// Maximum raw JSON nesting accepted while decoding a typed envelope; the decoded
/// `Value` is still bound by [`MAX_DEPTH`] (each typed level costs up to three raw levels).
const MAX_TYPED_RAW_DEPTH: usize = 4 + 3 * (MAX_DEPTH + 1);

/// Serialization format of a `to`/`from` stage.
///
/// `Json`/`Jsonl` are ordinary JSON (refuse values that cannot be represented exactly);
/// `JsonTyped`/`JsonlTyped` use the `{"cellbox":1,"value":NODE}` envelope and are lossless.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Format {
    Json,
    Jsonl,
    Text,
    JsonTyped,
    JsonlTyped,
    /// `to text --field FIELD`: render one named field of each record.
    TextField(String),
}

impl Format {
    /// True for the lossless typed-envelope encodings.
    pub fn is_typed(&self) -> bool {
        matches!(self, Self::JsonTyped | Self::JsonlTyped)
    }
}

/// One validated typed pipeline stage.
#[derive(Debug, Clone, PartialEq)]
pub enum Stage {
    Select(Vec<String>),
    Where { field: String, op: CompareOp, literal: Value },
    /// `sort-by [--reverse] [--frame] FIELD`: `frame` sorts each frame of a Frames stream.
    SortBy { field: String, reverse: bool, frame: bool },
    /// `take [--frame] N`: N rows, or N whole frames with `frame`.
    Take { n: usize, frame: bool },
    /// Consumes rows, produces bytes.
    To(Format),
    /// Consumes bytes (`Json`/`Jsonl`, plain or typed), produces rows.
    From(Format),
}

/// What feeds the first stage of a typed segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    /// A byte producer (`cat`, external, stdin, captured built-in).
    Bytes,
    /// A native producer of finite rows.
    Rows,
    /// A native producer of continuous frames.
    Frames,
}

/// What a stage sequence leaves at its tail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SinkKind {
    Rows,
    Bytes,
    Frames,
}

fn usage<T>(msg: String) -> Result<T, PipelineError> {
    Err(PipelineError::Usage(msg))
}

fn check_field_name(name: &str) -> Result<(), PipelineError> {
    if name.len() > MAX_FIELD_NAME_LEN {
        return Err(PipelineError::FieldNameTooLong);
    }
    let bad = name.is_empty()
        || name.starts_with('-')
        || name.chars().any(|c| {
            c.is_control()
                || c.is_whitespace()
                || matches!(c, '.' | '[' | ']' | '*' | '$' | '/' | '"' | '\'' | '\\')
        });
    if bad {
        return usage(format!("invalid field name '{}': fields are simple top-level names", name));
    }
    Ok(())
}

fn parse_exact_int<T: core::str::FromStr>(s: &str, allow_minus: bool) -> Option<T> {
    let first = s.as_bytes().first()?;
    let ok_first = first.is_ascii_digit() || (allow_minus && *first == b'-');
    if !ok_first || !s.bytes().skip(1).all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse::<T>().ok()
}

fn parse_where_literal(s: &str) -> Result<Value, PipelineError> {
    if let Some(d) = s.strip_prefix("int:") {
        return parse_exact_int::<i64>(d, true)
            .map(Value::Int)
            .ok_or_else(|| PipelineError::Usage(format!("literal '{}' is not an i64 decimal", s)));
    }
    if let Some(d) = s.strip_prefix("uint:") {
        return parse_exact_int::<u64>(d, false)
            .map(Value::UInt)
            .ok_or_else(|| PipelineError::Usage(format!("literal '{}' is not a u64 decimal", s)));
    }
    let v = decode_json(s.as_bytes()).map_err(|e| {
        PipelineError::Usage(format!(
            "literal '{}' is not a JSON scalar ({}); quote strings as '\"text\"'",
            s, e
        ))
    })?;
    match v {
        Value::List(_) | Value::Record(_) => {
            usage(format!("literal '{}' must be a scalar, not a list or record", s))
        }
        scalar => Ok(scalar),
    }
}

/// Parse ONE stage from argv (`argv[0]` is the command name). No effects.
pub fn parse_stage(argv: &[String]) -> Result<Stage, PipelineError> {
    let (name, args) = match argv.split_first() {
        Some(x) => x,
        None => return usage("empty pipeline stage".to_string()),
    };
    match name.as_str() {
        "select" => {
            if args.is_empty() {
                return usage("select: expected one or more field names".to_string());
            }
            for (i, a) in args.iter().enumerate() {
                check_field_name(a)?;
                if args[..i].contains(a) {
                    return usage(format!("select: duplicate field '{}'", a));
                }
            }
            Ok(Stage::Select(args.to_vec()))
        }
        "where" => {
            if args.len() != 3 {
                return usage(
                    "where: expected FIELD OP LITERAL (quote operators, e.g. where cpu_pct '>' uint:0)"
                        .to_string(),
                );
            }
            check_field_name(&args[0])?;
            let op = CompareOp::parse(&args[1]).ok_or_else(|| {
                PipelineError::Usage(format!(
                    "where: unknown operator '{}' (use == != < <= > >=, quoted)",
                    args[1]
                ))
            })?;
            let literal = parse_where_literal(&args[2])?;
            Ok(Stage::Where { field: args[0].clone(), op, literal })
        }
        "sort-by" => {
            let mut reverse = false;
            let mut frame = false;
            let mut field: Option<&String> = None;
            for a in args {
                match a.as_str() {
                    "--reverse" => {
                        if reverse {
                            return usage("sort-by: duplicate --reverse".to_string());
                        }
                        reverse = true;
                    }
                    "--frame" => {
                        if frame {
                            return usage("sort-by: duplicate --frame".to_string());
                        }
                        frame = true;
                    }
                    s if s.starts_with("--") => return usage(format!("sort-by: unknown option '{}'", s)),
                    _ => {
                        if field.is_some() {
                            return usage("sort-by: expected exactly one FIELD".to_string());
                        }
                        check_field_name(a)?;
                        field = Some(a);
                    }
                }
            }
            match field {
                Some(f) => Ok(Stage::SortBy { field: f.clone(), reverse, frame }),
                None => usage("sort-by: expected [--reverse] [--frame] FIELD".to_string()),
            }
        }
        "take" => {
            let (frame, count) = match args {
                [n] if n != "--frame" => (false, n),
                [flag, n] if flag == "--frame" && n != "--frame" => (true, n),
                _ if args.iter().filter(|a| *a == "--frame").count() > 1 => {
                    return usage("take: duplicate --frame".to_string())
                }
                _ => return usage("take: expected [--frame] N".to_string()),
            };
            match parse_exact_int::<usize>(count, false) {
                Some(n) => Ok(Stage::Take { n, frame }),
                None => usage(format!("take: '{}' is not a nonnegative decimal count", count)),
            }
        }
        "to" => {
            let (fmt_name, rest) = match args.split_first() {
                Some(x) => x,
                None => return usage("to: expected json, jsonl or text".to_string()),
            };
            let typed = |rest: &[String]| -> Result<bool, PipelineError> {
                match rest {
                    [] => Ok(false),
                    [f] if f == "--typed" => Ok(true),
                    _ => usage("to: only the optional --typed flag is accepted".to_string()),
                }
            };
            match fmt_name.as_str() {
                "json" => Ok(Stage::To(if typed(rest)? { Format::JsonTyped } else { Format::Json })),
                "jsonl" => Ok(Stage::To(if typed(rest)? { Format::JsonlTyped } else { Format::Jsonl })),
                "text" => match rest {
                    [] => Ok(Stage::To(Format::Text)),
                    [flag, field] if flag == "--field" => {
                        check_field_name(field)?;
                        Ok(Stage::To(Format::TextField(field.clone())))
                    }
                    _ => usage("to text: only [--field FIELD] is accepted".to_string()),
                },
                other => usage(format!("to: unknown format '{}' (json, jsonl, text)", other)),
            }
        }
        "from" => {
            let (fmt_name, rest) = match args.split_first() {
                Some(x) => x,
                None => return usage("from: expected json or jsonl".to_string()),
            };
            let typed = match rest {
                [] => false,
                [f] if f == "--typed" => true,
                _ => return usage("from: only the optional --typed flag is accepted".to_string()),
            };
            match (fmt_name.as_str(), typed) {
                ("json", false) => Ok(Stage::From(Format::Json)),
                ("json", true) => Ok(Stage::From(Format::JsonTyped)),
                ("jsonl", false) => Ok(Stage::From(Format::Jsonl)),
                ("jsonl", true) => Ok(Stage::From(Format::JsonlTyped)),
                (other, _) => usage(format!("from: unknown format '{}' (json, jsonl)", other)),
            }
        }
        other => usage(format!("unknown typed stage '{}'", other)),
    }
}

/// Static admission of a typed stage sequence fed by `input`. Returns what the sequence
/// leaves at its tail. Frames (spec §1) are refused by every Rows-only stage before any
/// producer exists: `sort-by`/`take` need `--frame` on Frames (and refuse it on Rows), and
/// only `to jsonl [--typed]` can encode a frame stream (continuous; D63-4).
pub fn validate_stage_sequence(input: Input, stages: &[Stage]) -> Result<SinkKind, PipelineError> {
    if stages.len() > MAX_STAGES {
        return Err(PipelineError::Sequence(format!(
            "pipeline exceeds {} stages",
            MAX_STAGES
        )));
    }
    let sequence = |msg: String| Err(PipelineError::Sequence(msg));
    let mut kind = match input {
        Input::Bytes => SinkKind::Bytes,
        Input::Rows => SinkKind::Rows,
        Input::Frames => SinkKind::Frames,
    };
    for (i, stage) in stages.iter().enumerate() {
        match stage {
            Stage::From(f) => {
                if !matches!(
                    f,
                    Format::Json | Format::Jsonl | Format::JsonTyped | Format::JsonlTyped
                ) {
                    return sequence("from accepts only json or jsonl".to_string());
                }
                if i != 0 || input != Input::Bytes {
                    return sequence(
                        "`from` must directly consume the byte producer at the start of the typed segment"
                            .to_string(),
                    );
                }
                kind = SinkKind::Rows;
            }
            Stage::To(format) => {
                match kind {
                    SinkKind::Bytes => {
                        return sequence("`to` needs rows; convert bytes with `from json|jsonl` first".to_string())
                    }
                    SinkKind::Frames if !matches!(format, Format::Jsonl | Format::JsonlTyped) => {
                        return sequence(
                            "input is a frame stream; only `to jsonl [--typed]` can encode frames".to_string(),
                        )
                    }
                    SinkKind::Rows | SinkKind::Frames => {}
                }
                if i + 1 != stages.len() {
                    return sequence(
                        "`to` is terminal for the typed segment and cannot feed another typed stage".to_string(),
                    );
                }
                kind = SinkKind::Bytes;
            }
            Stage::Select(_) | Stage::Where { .. } | Stage::SortBy { .. } | Stage::Take { .. } => {
                if kind == SinkKind::Bytes {
                    return sequence("bytes cannot feed a row stage without explicit `from json|jsonl`".to_string());
                }
                let (name, frame) = match stage {
                    Stage::SortBy { frame, .. } => ("sort-by", *frame),
                    Stage::Take { frame, .. } => ("take", *frame),
                    _ => continue,
                };
                match (kind, frame) {
                    (SinkKind::Frames, false) => {
                        return sequence(format!("{name}: input is a frame stream; use `{name} --frame`"))
                    }
                    (SinkKind::Rows, true) => {
                        return sequence(format!("{name} --frame: input is rows, not a frame stream"))
                    }
                    _ => {}
                }
            }
        }
    }
    Ok(kind)
}

// ─── Strict JSON decoding ────────────────────────────────────────────────────

struct JsonParser<'a> {
    b: &'a [u8],
    pos: usize,
    max_depth: usize,
}

impl<'a> JsonParser<'a> {
    fn new(b: &'a [u8], max_depth: usize) -> Self {
        Self { b, pos: 0, max_depth }
    }

    fn fail<T>(&self, msg: &str) -> Result<T, PipelineError> {
        Err(PipelineError::ParseError(format!("{} at byte {}", msg, self.pos)))
    }

    fn peek(&self) -> Option<u8> {
        self.b.get(self.pos).copied()
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    fn word(&mut self, word: &[u8], v: Value) -> Result<Value, PipelineError> {
        if self.b[self.pos..].starts_with(word) {
            self.pos += word.len();
            Ok(v)
        } else {
            self.fail("invalid literal")
        }
    }

    fn parse_value(&mut self, depth: usize) -> Result<Value, PipelineError> {
        if depth > self.max_depth {
            return Err(PipelineError::MaxDepthExceeded);
        }
        self.skip_ws();
        match self.peek() {
            None => self.fail("unexpected end of input"),
            Some(b'n') => self.word(b"null", Value::Null),
            Some(b't') => self.word(b"true", Value::Bool(true)),
            Some(b'f') => self.word(b"false", Value::Bool(false)),
            Some(b'"') => Ok(Value::String(self.parse_string()?)),
            Some(b'[') => self.parse_array(depth),
            Some(b'{') => self.parse_object(depth),
            Some(b'-' | b'0'..=b'9') => self.parse_number(),
            Some(_) => self.fail("unexpected character"),
        }
    }

    fn parse_array(&mut self, depth: usize) -> Result<Value, PipelineError> {
        self.pos += 1;
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(Value::List(items));
        }
        loop {
            items.push(self.parse_value(depth + 1)?);
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    return Ok(Value::List(items));
                }
                _ => return self.fail("expected ',' or ']'"),
            }
        }
    }

    fn parse_object(&mut self, depth: usize) -> Result<Value, PipelineError> {
        self.pos += 1;
        let mut fields: Vec<(String, Value)> = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(Value::Record(fields));
        }
        loop {
            self.skip_ws();
            if self.peek() != Some(b'"') {
                return self.fail("expected string field name");
            }
            let key = self.parse_string()?;
            if key.len() > MAX_FIELD_NAME_LEN {
                return Err(PipelineError::FieldNameTooLong);
            }
            self.skip_ws();
            if self.peek() != Some(b':') {
                return self.fail("expected ':'");
            }
            self.pos += 1;
            let val = self.parse_value(depth + 1)?;
            if fields.iter().any(|(k, _)| *k == key) {
                return Err(PipelineError::DuplicateField(key));
            }
            if fields.len() >= MAX_RECORD_FIELDS {
                return Err(PipelineError::TooManyFields);
            }
            fields.push((key, val));
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(Value::Record(fields));
                }
                _ => return self.fail("expected ',' or '}'"),
            }
        }
    }

    fn hex4(&mut self) -> Result<u32, PipelineError> {
        let mut v = 0u32;
        for _ in 0..4 {
            let d = match self.peek() {
                Some(c @ b'0'..=b'9') => c - b'0',
                Some(c @ b'a'..=b'f') => c - b'a' + 10,
                Some(c @ b'A'..=b'F') => c - b'A' + 10,
                _ => return self.fail("invalid \\u escape"),
            };
            v = v * 16 + d as u32;
            self.pos += 1;
        }
        Ok(v)
    }

    fn parse_string(&mut self) -> Result<String, PipelineError> {
        self.pos += 1;
        let mut buf: Vec<u8> = Vec::new();
        loop {
            let c = match self.peek() {
                Some(c) => c,
                None => return self.fail("unterminated string"),
            };
            match c {
                b'"' => {
                    self.pos += 1;
                    break;
                }
                b'\\' => {
                    self.pos += 1;
                    let e = match self.peek() {
                        Some(e) => e,
                        None => return self.fail("unterminated escape"),
                    };
                    self.pos += 1;
                    match e {
                        b'"' => buf.push(b'"'),
                        b'\\' => buf.push(b'\\'),
                        b'/' => buf.push(b'/'),
                        b'b' => buf.push(0x08),
                        b'f' => buf.push(0x0c),
                        b'n' => buf.push(b'\n'),
                        b'r' => buf.push(b'\r'),
                        b't' => buf.push(b'\t'),
                        b'u' => {
                            let hi = self.hex4()?;
                            let cp = if (0xD800..0xDC00).contains(&hi) {
                                if !self.b[self.pos..].starts_with(b"\\u") {
                                    return self.fail("lone high surrogate");
                                }
                                self.pos += 2;
                                let lo = self.hex4()?;
                                if !(0xDC00..0xE000).contains(&lo) {
                                    return self.fail("invalid low surrogate");
                                }
                                0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)
                            } else if (0xDC00..0xE000).contains(&hi) {
                                return self.fail("lone low surrogate");
                            } else {
                                hi
                            };
                            let ch = match char::from_u32(cp) {
                                Some(ch) => ch,
                                None => return self.fail("invalid code point"),
                            };
                            let mut tmp = [0u8; 4];
                            buf.extend_from_slice(ch.encode_utf8(&mut tmp).as_bytes());
                        }
                        _ => return self.fail("invalid escape"),
                    }
                }
                0..=0x1f => return self.fail("control character in string"),
                _ => {
                    buf.push(c);
                    self.pos += 1;
                }
            }
        }
        String::from_utf8(buf)
            .map_err(|_| PipelineError::ParseError("invalid UTF-8 in string".to_string()))
    }

    fn digits(&mut self) -> usize {
        let start = self.pos;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
        self.pos - start
    }

    fn parse_number(&mut self) -> Result<Value, PipelineError> {
        let start = self.pos;
        let negative = self.peek() == Some(b'-');
        if negative {
            self.pos += 1;
        }
        match self.peek() {
            Some(b'0') => {
                self.pos += 1;
                if matches!(self.peek(), Some(b'0'..=b'9')) {
                    return self.fail("leading zero in number");
                }
            }
            Some(b'1'..=b'9') => {
                self.digits();
            }
            _ => return self.fail("invalid number"),
        }
        let mut is_float = false;
        if self.peek() == Some(b'.') {
            is_float = true;
            self.pos += 1;
            if self.digits() == 0 {
                return self.fail("digits required after '.'");
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            is_float = true;
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if self.digits() == 0 {
                return self.fail("digits required in exponent");
            }
        }
        let text = core::str::from_utf8(&self.b[start..self.pos])
            .map_err(|_| PipelineError::ParseError("invalid number".to_string()))?;
        if is_float {
            let f: f64 = text
                .parse()
                .map_err(|_| PipelineError::ParseError("invalid number".to_string()))?;
            if !f.is_finite() {
                return Err(PipelineError::NonFiniteFloat);
            }
            return Ok(Value::Float(f));
        }
        if let Ok(n) = text.parse::<i64>() {
            return Ok(Value::Int(n));
        }
        if !negative {
            if let Ok(n) = text.parse::<u64>() {
                return Ok(Value::UInt(n));
            }
        }
        Err(PipelineError::PrecisionLoss(format!(
            "integer {} is outside the exact i64/u64 range",
            text
        )))
    }
}

fn parse_document(bytes: &[u8], max_depth: usize) -> Result<Value, PipelineError> {
    let mut p = JsonParser::new(bytes, max_depth);
    let v = p.parse_value(0)?;
    p.skip_ws();
    if p.pos != bytes.len() {
        return p.fail("trailing data after JSON value");
    }
    Ok(v)
}

/// Strict ordinary-JSON decoder for `from json`: exactly one value plus trailing whitespace.
pub fn decode_json(bytes: &[u8]) -> Result<Value, PipelineError> {
    if bytes.len() > MAX_JSON_DOCUMENT_BYTES {
        return Err(PipelineError::ParseError("document exceeds 4 MiB".to_string()));
    }
    parse_document(bytes, MAX_DEPTH)
}

/// Strict typed-envelope decoder for `from json --typed`.
pub fn decode_json_typed(bytes: &[u8]) -> Result<Value, PipelineError> {
    if bytes.len() > MAX_JSON_DOCUMENT_BYTES {
        return Err(PipelineError::ParseError("document exceeds 4 MiB".to_string()));
    }
    typed_envelope(parse_document(bytes, MAX_TYPED_RAW_DEPTH)?)
}

fn decode_jsonl_impl(bytes: &[u8], typed: bool) -> Result<Vec<Value>, PipelineError> {
    let mut rows = Vec::new();
    let mut start = 0usize;
    let mut line_no = 0usize;
    while start < bytes.len() {
        line_no += 1;
        let rest = &bytes[start..];
        let (line, terminated, consumed) = match rest.iter().position(|&c| c == b'\n') {
            Some(p) => (&rest[..p], true, p + 1),
            None => (rest, false, rest.len()),
        };
        start += consumed;
        if line.len() > MAX_RECORD_LINE_BYTES {
            return Err(PipelineError::RecordTooLarge);
        }
        let line = match line.split_last() {
            Some((b'\r', head)) if terminated => head,
            _ => line,
        };
        let decoded = if line.is_empty() {
            Err(PipelineError::ParseError("blank line".to_string()))
        } else if typed {
            parse_document(line, MAX_TYPED_RAW_DEPTH).and_then(typed_envelope)
        } else {
            parse_document(line, MAX_DEPTH)
        };
        match decoded {
            Ok(v) => rows.push(v),
            Err(PipelineError::ParseError(m)) => {
                return Err(PipelineError::ParseError(format!("line {}: {}", line_no, m)))
            }
            Err(e) => return Err(e),
        }
    }
    Ok(rows)
}

/// Strict ordinary-JSONL decoder: one row per LF-terminated line, final unterminated
/// nonempty line accepted, blank lines rejected, only the delimiter CR of CRLF stripped.
pub fn decode_jsonl(bytes: &[u8]) -> Result<Vec<Value>, PipelineError> {
    decode_jsonl_impl(bytes, false)
}

/// Strict typed-envelope JSONL decoder (`from jsonl --typed`).
pub fn decode_jsonl_typed(bytes: &[u8]) -> Result<Vec<Value>, PipelineError> {
    decode_jsonl_impl(bytes, true)
}

/// Decode the bytes of a `from` stage into rows. A JSON document that is a list
/// yields one row per element; any other single value is one row.
pub fn decode_rows(format: &Format, bytes: &[u8]) -> Result<Vec<Value>, PipelineError> {
    match format {
        Format::Json | Format::JsonTyped => {
            let v = if format.is_typed() { decode_json_typed(bytes)? } else { decode_json(bytes)? };
            Ok(match v {
                Value::List(items) => items,
                other => alloc::vec![other],
            })
        }
        Format::Jsonl => decode_jsonl(bytes),
        Format::JsonlTyped => decode_jsonl_typed(bytes),
        Format::Text | Format::TextField(_) => {
            usage("from accepts only json or jsonl".to_string())
        }
    }
}

fn typed_bad<T>(msg: &str) -> Result<T, PipelineError> {
    Err(PipelineError::ParseError(format!("typed envelope: {}", msg)))
}

fn typed_envelope(doc: Value) -> Result<Value, PipelineError> {
    let fields = match doc {
        Value::Record(f) if f.len() == 2 => f,
        _ => return typed_bad("expected {\"cellbox\":1,\"value\":NODE}"),
    };
    let mut node = None;
    let mut version_ok = false;
    for (k, v) in fields {
        match (k.as_str(), v) {
            ("cellbox", Value::Int(1)) => version_ok = true,
            ("value", v) => node = Some(v),
            _ => return typed_bad("unknown field or unsupported version"),
        }
    }
    let node = match (version_ok, node) {
        (true, Some(n)) => n,
        _ => return typed_bad("expected {\"cellbox\":1,\"value\":NODE}"),
    };
    let value = typed_node(node)?;
    value.validate()?;
    Ok(value)
}

fn decimal_payload<T: core::str::FromStr + fmt::Display>(p: Value) -> Result<T, PipelineError> {
    if let Value::String(s) = p {
        if let Ok(n) = s.parse::<T>() {
            if n.to_string() == s {
                return Ok(n);
            }
        }
    }
    typed_bad("integer payload must be a canonical decimal string")
}

fn hex_payload(p: Value) -> Result<Vec<u8>, PipelineError> {
    let s = match p {
        Value::String(s) => s,
        _ => return typed_bad("hex payload must be a string"),
    };
    let b = s.as_bytes();
    if b.len() % 2 != 0 {
        return typed_bad("hex payload has odd length");
    }
    let nib = |c: u8| match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        _ => None,
    };
    let mut out = Vec::with_capacity(b.len() / 2);
    for pair in b.chunks_exact(2) {
        match (nib(pair[0]), nib(pair[1])) {
            (Some(h), Some(l)) => out.push(h << 4 | l),
            _ => return typed_bad("hex payload must be lowercase hexadecimal"),
        }
    }
    Ok(out)
}

fn typed_node(node: Value) -> Result<Value, PipelineError> {
    let mut parts = match node {
        Value::List(p) if p.len() == 2 => p,
        _ => return typed_bad("node must be [TAG,PAYLOAD]"),
    };
    let payload = parts.pop().unwrap_or(Value::Null);
    let tag = match parts.pop() {
        Some(Value::String(t)) => t,
        _ => return typed_bad("node tag must be a string"),
    };
    match tag.as_str() {
        "null" => match payload {
            Value::Null => Ok(Value::Null),
            _ => typed_bad("null payload must be null"),
        },
        "bool" => match payload {
            Value::Bool(b) => Ok(Value::Bool(b)),
            _ => typed_bad("bool payload must be a boolean"),
        },
        "int" => decimal_payload::<i64>(payload).map(Value::Int),
        "uint" => decimal_payload::<u64>(payload).map(Value::UInt),
        "timestamp" => decimal_payload::<i64>(payload).map(Value::Timestamp),
        "duration" => decimal_payload::<i64>(payload).map(Value::Duration),
        "float" => match payload {
            Value::Float(f) if f.is_finite() => Ok(Value::Float(f)),
            Value::Int(n) if (MIN_SAFE_JSON_INT..=MAX_SAFE_JSON_INT).contains(&n) => {
                Ok(Value::Float(n as f64))
            }
            _ => typed_bad("float payload must be a finite exact JSON number"),
        },
        "string" => match payload {
            Value::String(s) => Ok(Value::String(s)),
            _ => typed_bad("string payload must be a string"),
        },
        "bytes" => hex_payload(payload).map(Value::Bytes),
        "path" => {
            let p = hex_payload(payload)?;
            if p.contains(&0) {
                return typed_bad("path contains NUL");
            }
            Ok(Value::Path(p))
        }
        "list" => match payload {
            Value::List(items) => {
                let mut out = Vec::with_capacity(items.len());
                for item in items {
                    out.push(typed_node(item)?);
                }
                Ok(Value::List(out))
            }
            _ => typed_bad("list payload must be an array of nodes"),
        },
        "record" => match payload {
            Value::List(pairs) => {
                if pairs.len() > MAX_RECORD_FIELDS {
                    return Err(PipelineError::TooManyFields);
                }
                let mut out: Vec<(String, Value)> = Vec::with_capacity(pairs.len());
                for pair in pairs {
                    let mut kv = match pair {
                        Value::List(kv) if kv.len() == 2 => kv,
                        _ => return typed_bad("record entry must be [NAME,NODE]"),
                    };
                    let val = kv.pop().unwrap_or(Value::Null);
                    let name = match kv.pop() {
                        Some(Value::String(n)) => n,
                        _ => return typed_bad("record field name must be a string"),
                    };
                    if name.len() > MAX_FIELD_NAME_LEN {
                        return Err(PipelineError::FieldNameTooLong);
                    }
                    if out.iter().any(|(k, _)| *k == name) {
                        return Err(PipelineError::DuplicateField(name));
                    }
                    out.push((name, typed_node(val)?));
                }
                Ok(Value::Record(out))
            }
            _ => typed_bad("record payload must be an array of [NAME,NODE]"),
        },
        _ => typed_bad("unknown tag"),
    }
}

// ─── Row encoding for `to` sinks ─────────────────────────────────────────────

const TYPED_ARRAY_OPEN: &[u8] = b"{\"cellbox\":1,\"value\":[\"list\",[";

fn write_text_value(v: &Value, field: Option<&str>, out: &mut Vec<u8>) -> Result<(), PipelineError> {
    match v {
        Value::String(s) => out.extend_from_slice(s.as_bytes()),
        Value::Bytes(b) | Value::Path(b) => out.extend_from_slice(b),
        Value::Record(_) => match field {
            Some(f) => {
                let inner = v
                    .get_field(f)
                    .ok_or_else(|| PipelineError::FieldNotFound(f.to_string()))?;
                return write_text_value(inner, None, out);
            }
            None => {
                return Err(PipelineError::UnsupportedType(
                    "record requires --field FIELD for text conversion".to_string(),
                ))
            }
        },
        other => out.extend_from_slice(value_to_text(other, None)?.as_bytes()),
    }
    Ok(())
}

/// Serialize one row (the body of [`encode_item`]). On error `out` is left unchanged.
///
/// * `Jsonl`/`JsonlTyped`: one LF-terminated line.
/// * `Json`/`JsonTyped`: one element of a single array/envelope; `first` writes the opener,
///   later rows a `,`; close with [`finish_encoding`].
/// * `Text`/`TextField`: String/Bytes/Path exact, scalars canonical text, LF-terminated.
fn encode_row(
    format: &Format,
    row: &Value,
    first: bool,
    out: &mut Vec<u8>,
) -> Result<(), PipelineError> {
    row.validate()?;
    match format {
        Format::Jsonl | Format::JsonlTyped => {
            let line = value_to_json(row, format.is_typed())?;
            out.extend_from_slice(line.as_bytes());
            out.push(b'\n');
        }
        Format::Json => {
            let s = value_to_json(row, false)?;
            out.push(if first { b'[' } else { b',' });
            out.extend_from_slice(s.as_bytes());
        }
        Format::JsonTyped => {
            let mut s = String::new();
            serialize_typed_node(row, &mut s)?;
            if s.len() > MAX_RECORD_LINE_BYTES {
                return Err(PipelineError::RecordTooLarge);
            }
            if first {
                out.extend_from_slice(TYPED_ARRAY_OPEN);
            } else {
                out.push(b',');
            }
            out.extend_from_slice(s.as_bytes());
        }
        Format::Text | Format::TextField(_) => {
            let field = match format {
                Format::TextField(f) => Some(f.as_str()),
                _ => None,
            };
            let start = out.len();
            if let Err(e) = write_text_value(row, field, out) {
                out.truncate(start);
                return Err(e);
            }
            out.push(b'\n');
        }
    }
    Ok(())
}

/// Terminate an encoding started with [`encode_row`]. `any_rows` says whether at least one row
/// was encoded (so `first == true` was consumed).
pub fn finish_encoding(format: &Format, any_rows: bool, out: &mut Vec<u8>) {
    match format {
        Format::Json => out.extend_from_slice(if any_rows { b"]\n" } else { b"[]\n" }),
        Format::JsonTyped => {
            if any_rows {
                out.extend_from_slice(b"]]}\n");
            } else {
                out.extend_from_slice(b"{\"cellbox\":1,\"value\":[\"list\",[]]}\n");
            }
        }
        Format::Jsonl | Format::JsonlTyped | Format::Text | Format::TextField(_) => {}
    }
}

fn envelope_field(name: &str, value: Value) -> (String, Value) {
    (String::from(name), value)
}

/// The JSONL envelope of one Frames item (D63-2): a `Record` that owns the row, so the
/// existing serializers and bounds (line, depth — envelope included) apply unchanged.
fn frame_envelope(item: Item) -> Value {
    let tag = |name: &str| envelope_field("frame", Value::String(String::from(name)));
    Value::Record(match item {
        Item::Begin(begin) => {
            let mut fields = alloc::vec![
                tag("begin"),
                envelope_field("sample_id", Value::UInt(begin.sample_id)),
                envelope_field("monotonic_ns", Value::UInt(begin.monotonic_ns)),
            ];
            if let Some(meta) = begin.meta {
                fields.push(envelope_field("meta", meta));
            }
            fields
        }
        Item::Row(row) => alloc::vec![tag("row"), envelope_field("value", row)],
        Item::End(end) => alloc::vec![
            tag("end"),
            envelope_field("complete", Value::Bool(end.complete)),
            envelope_field(
                "unavailable_fields",
                Value::List(end.unavailable_fields.into_iter().map(Value::String).collect()),
            ),
            envelope_field("dropped", Value::UInt(end.dropped)),
        ],
    })
}

/// Serialize one item for a `to` sink. On error `out` is left unchanged.
///
/// In a Rows stream an item is a row and encodes as described for each format (`Jsonl`
/// lines, one `Json` array element — `first` writes the opener, later rows a `,`, close with
/// [`finish_encoding`] — or LF-terminated `Text`). In a Frames stream only `Jsonl`/
/// `JsonlTyped` are admitted; each item becomes one `{"frame":"begin"|"row"|"end",…}`
/// envelope line. A marker in a Rows stream or a frame in another format is a
/// [`PipelineError::Sequence`] (admission and [`check_items`] already exclude both).
pub fn encode_item(
    format: &Format,
    category: Category,
    item: Item,
    first: bool,
    out: &mut Vec<u8>,
) -> Result<(), PipelineError> {
    match (category, item) {
        (Category::Rows, Item::Row(row)) => encode_row(format, &row, first, out),
        (Category::Rows, _) => Err(PipelineError::Sequence("frame marker in a rows stream".to_string())),
        (Category::Frames, item) => {
            if !matches!(format, Format::Jsonl | Format::JsonlTyped) {
                return Err(PipelineError::Sequence(
                    "input is a frame stream; only `to jsonl [--typed]` can encode frames".to_string(),
                ));
            }
            encode_row(format, &frame_envelope(item), first, out)
        }
    }
}

// ─── Pull-based bounded item runtime ─────────────────────────────────────────

/// Start of one spec Frame (spec §1): `sample_id` strictly increases within a stream and
/// `meta`, when present, is a producer `Record` (schema, totals, units) — never authority.
#[derive(Debug, Clone, PartialEq)]
pub struct FrameBegin {
    pub sample_id: u64,
    pub monotonic_ns: u64,
    pub meta: Option<Value>,
}

/// End of one spec Frame: `complete` is false when the producer dropped rows, `dropped`
/// counts them, `unavailable_fields` names fields the producer could not supply.
#[derive(Debug, Clone, PartialEq)]
pub struct FrameEnd {
    pub complete: bool,
    pub unavailable_fields: Vec<String>,
    pub dropped: u64,
}

/// One element of a typed stream: a row, or a frame marker in a Frames stream.
#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Begin(FrameBegin),
    Row(Value),
    End(FrameEnd),
}

/// What a producer emits: finite Rows, or continuous Frames (Begin, Row*, End)*.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Rows,
    Frames,
}

/// A pull-based producer of items. Closing must propagate upstream.
pub trait RowSource {
    /// Next item, `Ok(None)` at end. After an error or `close` it keeps returning `Ok(None)`.
    fn next_item(&mut self) -> Result<Option<Item>, PipelineError>;
    /// Idempotent; releases the upstream chain.
    fn close(&mut self);
    /// The producer's completion status (`rg`: 0 matched, 1 no match, 2 error), read by the
    /// driver after the chain is closed. `None` when the producer reports none (the default)
    /// or did not run to completion (e.g. interrupted). Stages forward their upstream's value.
    fn completion(&self) -> Option<i32> {
        None
    }
}

/// A finite in-memory source of rows (or, for fixtures, of arbitrary items).
pub struct VecSource {
    items: VecDeque<Item>,
}

impl VecSource {
    pub fn new(rows: Vec<Value>) -> Self {
        Self { items: rows.into_iter().map(Item::Row).collect() }
    }

    pub fn from_items(items: Vec<Item>) -> Self {
        Self { items: VecDeque::from(items) }
    }
}

impl RowSource for VecSource {
    fn next_item(&mut self) -> Result<Option<Item>, PipelineError> {
        Ok(self.items.pop_front())
    }

    fn close(&mut self) {
        self.items = VecDeque::new();
    }
}

/// How many [`InterruptLatch::check`] calls share one real poll.
const LATCH_CHECK_PERIOD: u32 = 16;

/// Sticky interrupt flag shared by every party of one typed job (one latch per job).
///
/// The job's producer calls [`check`](Self::check) (a `Cell` read, with one real poll every
/// 16th call); sinks and external-stage pumps call [`poll_now`](Self::poll_now) at their own
/// cadence. Whoever observes the interrupt first latches it, so no party can consume an
/// interrupt the others then miss.
pub struct InterruptLatch {
    set: Cell<bool>,
    checks: Cell<u32>,
    poll: RefCell<Box<dyn FnMut() -> bool>>,
}

impl InterruptLatch {
    /// A latch whose real poll is `poll` (`true` = interrupt observed).
    pub fn new(poll: Box<dyn FnMut() -> bool>) -> Rc<Self> {
        Rc::new(Self { set: Cell::new(false), checks: Cell::new(0), poll: RefCell::new(poll) })
    }

    /// A latch that never polls (no session); it is set only by [`set`](Self::set).
    pub fn inert() -> Rc<Self> {
        Self::new(Box::new(|| false))
    }

    /// Whether an interrupt has been latched. Never polls.
    pub fn is_set(&self) -> bool {
        self.set.get()
    }

    /// Poll once unless already set, latching an observed interrupt. Returns [`is_set`](Self::is_set).
    pub fn poll_now(&self) -> bool {
        if !self.set.get() && (self.poll.borrow_mut())() {
            self.set.set(true);
        }
        self.set.get()
    }

    /// Producer check: `true` once set; otherwise every 16th call (16, 32, …) does
    /// [`poll_now`](Self::poll_now). Returns [`is_set`](Self::is_set).
    pub fn check(&self) -> bool {
        if self.set.get() {
            return true;
        }
        let n = (self.checks.get() + 1) % LATCH_CHECK_PERIOD;
        self.checks.set(n);
        if n == 0 {
            return self.poll_now();
        }
        false
    }

    /// Latch the interrupt explicitly.
    pub fn set(&self) {
        self.set.set(true);
    }
}

/// Owned upstream handle that closes itself on EOF, error and explicit close. The source's
/// [`RowSource::completion`] is read after its `close` and before it is dropped, and kept.
struct Upstream {
    src: Option<Box<dyn RowSource>>,
    completion: Option<i32>,
}

impl Upstream {
    fn new(src: Box<dyn RowSource>) -> Self {
        Self { src: Some(src), completion: None }
    }

    fn pull(&mut self) -> Result<Option<Item>, PipelineError> {
        let res = match self.src.as_mut() {
            Some(s) => s.next_item(),
            None => return Ok(None),
        };
        if !matches!(res, Ok(Some(_))) {
            self.close();
        }
        res
    }

    /// Close upstream and return `error` (every stage failure releases the chain first).
    fn fail<T>(&mut self, error: PipelineError) -> Result<T, PipelineError> {
        self.close();
        Err(error)
    }

    fn close(&mut self) {
        if let Some(mut s) = self.src.take() {
            s.close();
            self.completion = s.completion();
        }
    }

    /// The live source's completion while it is held, else the value kept at close.
    fn completion(&self) -> Option<i32> {
        match &self.src {
            Some(s) => s.completion(),
            None => self.completion,
        }
    }
}

fn marker_in_rows(stage: &str) -> PipelineError {
    PipelineError::Sequence(format!("{}: frame marker in a rows stream", stage))
}

fn sequence_error(msg: &str) -> PipelineError {
    PipelineError::Sequence(msg.to_string())
}

/// Order guard applied once at a producer (see [`check_items`]).
struct OrderGuard {
    up: Upstream,
    category: Category,
    in_frame: bool,
    last_sample: Option<u64>,
}

impl OrderGuard {
    fn check(&mut self, item: &Option<Item>) -> Result<(), PipelineError> {
        match (self.category, item) {
            (_, None) if self.in_frame => Err(sequence_error("frame stream ended inside a frame")),
            (_, None) | (Category::Rows, Some(Item::Row(_))) => Ok(()),
            (Category::Rows, Some(_)) => Err(sequence_error("frame marker in a rows stream")),
            (Category::Frames, Some(Item::Begin(begin))) => {
                if self.in_frame {
                    return Err(sequence_error("frame begins inside another frame"));
                }
                if self.last_sample.is_some_and(|last| begin.sample_id <= last) {
                    return Err(sequence_error("frame sample_id did not strictly increase"));
                }
                self.in_frame = true;
                self.last_sample = Some(begin.sample_id);
                Ok(())
            }
            (Category::Frames, Some(Item::Row(_))) if !self.in_frame => {
                Err(sequence_error("row outside a frame"))
            }
            (Category::Frames, Some(Item::Row(_))) => Ok(()),
            (Category::Frames, Some(Item::End(_))) => {
                if !self.in_frame {
                    return Err(sequence_error("frame end without a begin"));
                }
                self.in_frame = false;
                Ok(())
            }
        }
    }
}

impl RowSource for OrderGuard {
    fn next_item(&mut self) -> Result<Option<Item>, PipelineError> {
        let item = self.up.pull()?;
        if let Err(error) = self.check(&item) {
            self.in_frame = false;
            return self.up.fail(error);
        }
        Ok(item)
    }

    fn close(&mut self) {
        self.in_frame = false;
        self.up.close();
    }

    fn completion(&self) -> Option<i32> {
        self.up.completion()
    }
}

/// Wrap a producer with the item-order guard for its `category`. Applied once at the
/// producer; every stage and sink downstream relies on the order without re-checking it.
/// A violation is a [`PipelineError::Sequence`] that closes the producer: any marker in a
/// Rows stream; a Row outside a frame, a nested Begin or an End without a Begin; a stream
/// that ends inside a frame; a `sample_id` that does not strictly increase.
pub fn check_items(source: Box<dyn RowSource>, category: Category) -> Box<dyn RowSource> {
    Box::new(OrderGuard { up: Upstream::new(source), category, in_frame: false, last_sample: None })
}

/// `select`: projects rows; frame markers pass through unchanged.
struct SelectSource {
    up: Upstream,
    fields: Vec<String>,
}

impl RowSource for SelectSource {
    fn next_item(&mut self) -> Result<Option<Item>, PipelineError> {
        let row = match self.up.pull()? {
            Some(Item::Row(r)) => r,
            marker => return Ok(marker),
        };
        let mut recs = match row {
            Value::Record(r) => r,
            _ => return self.up.fail(PipelineError::TypeMismatch),
        };
        let mut out = Vec::with_capacity(self.fields.len());
        for f in &self.fields {
            match recs.iter().position(|(k, _)| k == f) {
                Some(i) => out.push(recs.swap_remove(i)),
                None => return self.up.fail(PipelineError::FieldNotFound(f.clone())),
            }
        }
        Ok(Some(Item::Row(Value::Record(out))))
    }

    fn close(&mut self) {
        self.up.close();
    }

    fn completion(&self) -> Option<i32> {
        self.up.completion()
    }
}

/// `where`: filters rows; frame markers pass through unchanged (filtering is not truncation).
struct WhereSource {
    up: Upstream,
    field: String,
    op: CompareOp,
    literal: Value,
}

impl RowSource for WhereSource {
    fn next_item(&mut self) -> Result<Option<Item>, PipelineError> {
        loop {
            let row = match self.up.pull()? {
                Some(Item::Row(r)) => r,
                marker => return Ok(marker),
            };
            match filter_record(&row, &self.field, self.op, &self.literal) {
                Ok(true) => return Ok(Some(Item::Row(row))),
                Ok(false) => {}
                Err(e) => return self.up.fail(e),
            }
        }
    }

    fn close(&mut self) {
        self.up.close();
    }

    fn completion(&self) -> Option<i32> {
        self.up.completion()
    }
}

/// `take N` over rows, or `take --frame N` over frames (counting Ends).
struct TakeSource {
    up: Upstream,
    remaining: usize,
    frame: bool,
}

impl RowSource for TakeSource {
    fn next_item(&mut self) -> Result<Option<Item>, PipelineError> {
        if self.remaining == 0 {
            self.up.close();
            return Ok(None);
        }
        let item = match self.up.pull()? {
            Some(item) => item,
            None => {
                self.remaining = 0;
                return Ok(None);
            }
        };
        let counts = match (&item, self.frame) {
            (Item::Row(_), false) | (Item::End(_), true) => true,
            (_, true) => false,
            (_, false) => {
                self.remaining = 0;
                return self.up.fail(marker_in_rows("take"));
            }
        };
        if counts {
            self.remaining -= 1;
            if self.remaining == 0 {
                self.up.close();
            }
        }
        Ok(Some(item))
    }

    fn close(&mut self) {
        self.remaining = 0;
        self.up.close();
    }

    fn completion(&self) -> Option<i32> {
        self.up.completion()
    }
}

/// Pull rows into a sort buffer until `stop` says an item ends the run, under the global
/// sort bounds. Returns the buffered rows and the item that ended them (`None` at EOF).
fn buffer_rows(
    up: &mut Upstream,
    first: Option<Value>,
    mut stop: impl FnMut(&Item) -> bool,
) -> Result<(Vec<Value>, Option<Item>), PipelineError> {
    let mut rows: Vec<Value> = Vec::new();
    let mut bytes = 0usize;
    let mut push = |rows: &mut Vec<Value>, row: Value| {
        bytes = bytes.saturating_add(row.payload_bytes());
        if rows.len() >= MAX_SORT_RECORDS || bytes > MAX_SORT_PAYLOAD_BYTES {
            return Err(PipelineError::SortLimitExceeded);
        }
        rows.push(row);
        Ok(())
    };
    if let Some(row) = first {
        push(&mut rows, row)?;
    }
    loop {
        match up.pull()? {
            Some(Item::Row(row)) => push(&mut rows, row)?,
            Some(item) if stop(&item) => return Ok((rows, Some(item))),
            Some(_) => return Err(sequence_error("unexpected frame marker in a sort buffer")),
            None => return Ok((rows, None)),
        }
    }
}

/// `sort-by FIELD` over a whole finite Rows stream.
struct SortSource {
    up: Upstream,
    field: String,
    reverse: bool,
    sorted: Option<alloc::vec::IntoIter<Value>>,
}

impl SortSource {
    fn fill(&mut self) -> Result<(), PipelineError> {
        let (mut rows, _) = buffer_rows(&mut self.up, None, |_| false)?;
        sort_records_by(&mut rows, &self.field, self.reverse)?;
        self.sorted = Some(rows.into_iter());
        Ok(())
    }
}

impl RowSource for SortSource {
    fn next_item(&mut self) -> Result<Option<Item>, PipelineError> {
        if self.sorted.is_none() {
            if let Err(e) = self.fill() {
                self.sorted = Some(Vec::new().into_iter());
                return self.up.fail(e);
            }
        }
        Ok(self.sorted.as_mut().and_then(|it| it.next()).map(Item::Row))
    }

    fn close(&mut self) {
        self.up.close();
        self.sorted = Some(Vec::new().into_iter());
    }

    fn completion(&self) -> Option<i32> {
        self.up.completion()
    }
}

/// `sort-by --frame FIELD`: Begin is emitted as received; the frame's rows are buffered up
/// to its End (sort bounds), sorted and emitted, then the End. Nothing past End is pulled
/// before the frame is emitted, and no history outlives the frame.
struct SortFrameSource {
    up: Upstream,
    field: String,
    reverse: bool,
    ready: VecDeque<Item>,
}

impl SortFrameSource {
    fn sort_frame(&mut self, first: Value) -> Result<(), PipelineError> {
        let (mut rows, end) = buffer_rows(&mut self.up, Some(first), |item| matches!(item, Item::End(_)))?;
        let end = end.ok_or_else(|| sequence_error("frame stream ended inside a frame"))?;
        sort_records_by(&mut rows, &self.field, self.reverse)?;
        self.ready.extend(rows.into_iter().map(Item::Row));
        self.ready.push_back(end);
        Ok(())
    }
}

impl RowSource for SortFrameSource {
    fn next_item(&mut self) -> Result<Option<Item>, PipelineError> {
        if let Some(item) = self.ready.pop_front() {
            return Ok(Some(item));
        }
        match self.up.pull()? {
            Some(Item::Row(first)) => {
                if let Err(e) = self.sort_frame(first) {
                    self.ready.clear();
                    return self.up.fail(e);
                }
                Ok(self.ready.pop_front())
            }
            other => Ok(other),
        }
    }

    fn close(&mut self) {
        self.ready.clear();
        self.up.close();
    }

    fn completion(&self) -> Option<i32> {
        self.up.completion()
    }
}

/// Wrap `input` with a row stage (Select/Where/SortBy/Take). `Select`/`Where` pass frame
/// markers through unchanged. `SortBy` buffers at most [`MAX_SORT_RECORDS`] rows /
/// [`MAX_SORT_PAYLOAD_BYTES`] (the whole stream, or one frame with `--frame`) then errors.
/// `Take` closes upstream as soon as the nth row (or nth frame End) is delivered (`n == 0`
/// closes immediately without pulling). `To`/`From` are not row stages and are refused
/// (the input is closed first).
pub fn apply_row_stage(
    input: Box<dyn RowSource>,
    stage: &Stage,
) -> Result<Box<dyn RowSource>, PipelineError> {
    let mut up = Upstream::new(input);
    match stage {
        Stage::Select(fields) => Ok(Box::new(SelectSource { up, fields: fields.clone() })),
        Stage::Where { field, op, literal } => Ok(Box::new(WhereSource {
            up,
            field: field.clone(),
            op: *op,
            literal: literal.clone(),
        })),
        Stage::SortBy { field, reverse, frame: false } => Ok(Box::new(SortSource {
            up,
            field: field.clone(),
            reverse: *reverse,
            sorted: None,
        })),
        Stage::SortBy { field, reverse, frame: true } => Ok(Box::new(SortFrameSource {
            up,
            field: field.clone(),
            reverse: *reverse,
            ready: VecDeque::new(),
        })),
        Stage::Take { n, frame } => {
            if *n == 0 {
                up.close();
            }
            Ok(Box::new(TakeSource { up, remaining: *n, frame: *frame }))
        }
        Stage::To(_) | Stage::From(_) => {
            up.close();
            Err(PipelineError::Sequence(
                "`to`/`from` are byte conversions, not row stages".to_string(),
            ))
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_value_comparison_order() {
        // Null < present
        assert_eq!(
            compare_values(&Value::Null, &Value::Int(0)).unwrap(),
            Ordering::Less
        );
        assert_eq!(
            compare_values(&Value::Null, &Value::Null).unwrap(),
            Ordering::Equal
        );
        assert_eq!(
            compare_values(&Value::Int(0), &Value::Null).unwrap(),
            Ordering::Greater
        );

        // Int and UInt exact comparison without float coercion
        assert_eq!(
            compare_values(&Value::Int(-5), &Value::UInt(10)).unwrap(),
            Ordering::Less
        );
        assert_eq!(
            compare_values(&Value::Int(100), &Value::UInt(100)).unwrap(),
            Ordering::Equal
        );
        assert_eq!(
            compare_values(&Value::UInt(100), &Value::Int(50)).unwrap(),
            Ordering::Greater
        );

        // String lexical
        assert_eq!(
            compare_values(
                &Value::String("apple".into()),
                &Value::String("banana".into())
            )
            .unwrap(),
            Ordering::Less
        );

        // Type mismatch
        assert_eq!(
            compare_values(&Value::Int(1), &Value::String("1".into())).unwrap_err(),
            PipelineError::TypeMismatch
        );
    }

    #[test]
    fn test_select_transform() {
        let rec = Value::Record(alloc::vec![
            ("name".into(), Value::String("alice".into())),
            ("age".into(), Value::UInt(30)),
            ("city".into(), Value::String("hanoi".into())),
        ]);

        let sel = select_fields(&rec, &["city", "name"]).unwrap();
        assert_eq!(
            sel,
            Value::Record(alloc::vec![
                ("city".into(), Value::String("hanoi".into())),
                ("name".into(), Value::String("alice".into())),
            ])
        );

        // Missing field
        assert_eq!(
            select_fields(&rec, &["missing"]).unwrap_err(),
            PipelineError::FieldNotFound("missing".into())
        );
    }

    #[test]
    fn test_where_filter_and_sort() {
        let mut records = alloc::vec![
            Value::Record(alloc::vec![
                ("id".into(), Value::UInt(3)),
                ("score".into(), Value::Int(90)),
            ]),
            Value::Record(alloc::vec![
                ("id".into(), Value::UInt(1)),
                ("score".into(), Value::Int(100)),
            ]),
            Value::Record(alloc::vec![
                ("id".into(), Value::UInt(2)),
                ("score".into(), Value::Int(80)),
            ]),
        ];

        // Filter: score >= 90
        assert!(filter_record(&records[0], "score", CompareOp::GreaterOrEqual, &Value::Int(90)).unwrap());
        assert!(!filter_record(&records[2], "score", CompareOp::GreaterOrEqual, &Value::Int(90)).unwrap());

        // Sort by id ascending
        sort_records_by(&mut records, "id", false).unwrap();
        assert_eq!(records[0].get_field("id"), Some(&Value::UInt(1)));
        assert_eq!(records[1].get_field("id"), Some(&Value::UInt(2)));
        assert_eq!(records[2].get_field("id"), Some(&Value::UInt(3)));

        // Sort by score descending (reverse)
        sort_records_by(&mut records, "score", true).unwrap();
        assert_eq!(records[0].get_field("score"), Some(&Value::Int(100)));
        assert_eq!(records[1].get_field("score"), Some(&Value::Int(90)));
        assert_eq!(records[2].get_field("score"), Some(&Value::Int(80)));
    }

    #[test]
    fn test_typed_json_serialization() {
        let val = Value::Record(alloc::vec![
            ("tag".into(), Value::String("test".into())),
            ("raw".into(), Value::Bytes(alloc::vec![0xde, 0xad, 0xbe, 0xef])),
            ("big".into(), Value::UInt(18446744073709551615)),
        ]);

        let typed_json = value_to_json(&val, true).unwrap();
        assert!(typed_json.starts_with("{\"cellbox\":1,\"value\":[\"record\",["));
        assert!(typed_json.contains("[\"bytes\",\"deadbeef\"]"));
        assert!(typed_json.contains("[\"uint\",\"18446744073709551615\"]"));

        // Untyped mode fails on Bytes without precision loss
        let err = value_to_json(&val, false).unwrap_err();
        assert!(matches!(err, PipelineError::UnsupportedType(_)));
    }
}

#[cfg(test)]
mod stage_tests {
    use super::*;
    use alloc::rc::Rc;
    use alloc::vec;
    use core::cell::Cell;

    fn rec(fields: &[(&str, Value)]) -> Value {
        Value::Record(fields.iter().map(|(k, v)| (k.to_string(), v.clone())).collect())
    }

    fn argv(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    /// Test helper: the next item, which must be a row.
    trait NextRow {
        fn next_row(&mut self) -> Result<Option<Value>, PipelineError>;
    }

    impl<T: RowSource + ?Sized> NextRow for T {
        fn next_row(&mut self) -> Result<Option<Value>, PipelineError> {
            match self.next_item()? {
                Some(Item::Row(row)) => Ok(Some(row)),
                Some(marker) => panic!("unexpected marker {:?}", marker),
                None => Ok(None),
            }
        }
    }

    fn drain(mut src: Box<dyn RowSource>) -> Result<Vec<Value>, PipelineError> {
        let mut out = Vec::new();
        while let Some(r) = src.next_row()? {
            out.push(r);
        }
        Ok(out)
    }

    /// Endless/limited counting producer observing pulls and closes.
    struct Counting {
        next: u64,
        limit: Option<u64>,
        pulled: Rc<Cell<u64>>,
        closed: Rc<Cell<u32>>,
    }

    impl Counting {
        fn new(limit: Option<u64>) -> (Self, Rc<Cell<u64>>, Rc<Cell<u32>>) {
            let pulled = Rc::new(Cell::new(0));
            let closed = Rc::new(Cell::new(0));
            (
                Self { next: 0, limit, pulled: pulled.clone(), closed: closed.clone() },
                pulled,
                closed,
            )
        }
    }

    impl RowSource for Counting {
        fn next_item(&mut self) -> Result<Option<Item>, PipelineError> {
            if self.closed.get() > 0 {
                return Ok(None);
            }
            if self.limit.map_or(false, |l| self.next >= l) {
                return Ok(None);
            }
            let n = self.next;
            self.next += 1;
            self.pulled.set(self.pulled.get() + 1);
            Ok(Some(Item::Row(rec(&[("n", Value::UInt(n))]))))
        }

        fn close(&mut self) {
            self.closed.set(self.closed.get() + 1);
        }
    }

    #[test]
    fn numeric_comparison_is_exact_and_never_coerces_floats() {
        assert_eq!(
            compare_values(&Value::UInt(u64::MAX), &Value::Int(i64::MAX)).unwrap(),
            Ordering::Greater
        );
        assert_eq!(
            compare_values(&Value::Int(-1), &Value::UInt(u64::MAX)).unwrap(),
            Ordering::Less
        );
        assert_eq!(
            compare_values(&Value::Int(i64::MAX), &Value::UInt(i64::MAX as u64)).unwrap(),
            Ordering::Equal
        );
        let r = rec(&[("x", Value::Float(1.0))]);
        assert_eq!(
            filter_record(&r, "x", CompareOp::Equal, &Value::Int(1)).unwrap_err(),
            PipelineError::TypeMismatch
        );
        let r = rec(&[("x", Value::Int(1))]);
        assert_eq!(
            filter_record(&r, "x", CompareOp::Equal, &Value::Float(1.0)).unwrap_err(),
            PipelineError::TypeMismatch
        );
        let r = rec(&[("x", Value::UInt(u64::MAX))]);
        assert!(filter_record(&r, "x", CompareOp::GreaterThan, &Value::Int(i64::MAX)).unwrap());
        assert_eq!(
            filter_record(&rec(&[("s", Value::String("1".into()))]), "s", CompareOp::Equal, &Value::Int(1))
                .unwrap_err(),
            PipelineError::TypeMismatch
        );
    }

    #[test]
    fn null_is_not_zero_and_never_orders() {
        let r = rec(&[("x", Value::Null)]);
        assert!(filter_record(&r, "x", CompareOp::Equal, &Value::Null).unwrap());
        assert!(!filter_record(&r, "x", CompareOp::NotEqual, &Value::Null).unwrap());
        assert!(!filter_record(&r, "x", CompareOp::Equal, &Value::Int(0)).unwrap());
        assert!(filter_record(&r, "x", CompareOp::NotEqual, &Value::Int(0)).unwrap());
        for op in [CompareOp::LessThan, CompareOp::LessOrEqual, CompareOp::GreaterThan, CompareOp::GreaterOrEqual] {
            assert!(!filter_record(&r, "x", op, &Value::Int(0)).unwrap());
            assert!(!filter_record(&r, "x", op, &Value::Null).unwrap());
            let z = rec(&[("x", Value::Int(0))]);
            assert!(!filter_record(&z, "x", op, &Value::Null).unwrap());
        }
        assert!(matches!(
            filter_record(&rec(&[("b", Value::Bool(true))]), "b", CompareOp::LessThan, &Value::Bool(false)),
            Err(PipelineError::UnsupportedType(_))
        ));
        assert!(filter_record(&rec(&[("b", Value::Bool(true))]), "b", CompareOp::Equal, &Value::Bool(true)).unwrap());
    }

    #[test]
    fn parse_stage_accepts_exact_vocabulary() {
        assert_eq!(
            parse_stage(&argv(&["select", "pid", "name"])).unwrap(),
            Stage::Select(argv(&["pid", "name"]))
        );
        assert_eq!(
            parse_stage(&argv(&["where", "state", "==", "\"running\""])).unwrap(),
            Stage::Where {
                field: "state".into(),
                op: CompareOp::Equal,
                literal: Value::String("running".into())
            }
        );
        assert_eq!(
            parse_stage(&argv(&["where", "cpu_tenths", ">", "uint:0"])).unwrap(),
            Stage::Where { field: "cpu_tenths".into(), op: CompareOp::GreaterThan, literal: Value::UInt(0) }
        );
        assert_eq!(
            parse_stage(&argv(&["sort-by", "--reverse", "pid"])).unwrap(),
            Stage::SortBy { field: "pid".into(), reverse: true, frame: false }
        );
        assert_eq!(parse_stage(&argv(&["sort-by", "pid"])).unwrap(), Stage::SortBy { field: "pid".into(), reverse: false, frame: false });
        assert_eq!(parse_stage(&argv(&["take", "0"])).unwrap(), Stage::Take { n: 0, frame: false });
        assert_eq!(parse_stage(&argv(&["take", "12"])).unwrap(), Stage::Take { n: 12, frame: false });
        assert_eq!(parse_stage(&argv(&["to", "json"])).unwrap(), Stage::To(Format::Json));
        assert_eq!(parse_stage(&argv(&["to", "jsonl", "--typed"])).unwrap(), Stage::To(Format::JsonlTyped));
        assert_eq!(parse_stage(&argv(&["to", "json", "--typed"])).unwrap(), Stage::To(Format::JsonTyped));
        assert_eq!(parse_stage(&argv(&["to", "text"])).unwrap(), Stage::To(Format::Text));
        assert_eq!(
            parse_stage(&argv(&["to", "text", "--field", "name"])).unwrap(),
            Stage::To(Format::TextField("name".into()))
        );
        assert_eq!(parse_stage(&argv(&["from", "json"])).unwrap(), Stage::From(Format::Json));
        assert_eq!(parse_stage(&argv(&["from", "jsonl", "--typed"])).unwrap(), Stage::From(Format::JsonlTyped));
    }

    #[test]
    fn parse_stage_rejects_bad_arguments() {
        let bad: &[&[&str]] = &[
            &[],
            &["select"],
            &["select", "a", "a"],
            &["select", "a.b"],
            &["select", "--x"],
            &["where", "a", "==="],
            &["where", "a", "==", "1", "2"],
            &["where", "a", "=="],
            &["where", "a", "==", "running"],
            &["where", "a", "==", "[1]"],
            &["where", "a", "==", "{}"],
            &["where", "a", "==", "int:1.5"],
            &["where", "a", "==", "int:+1"],
            &["where", "a", "==", "uint:-1"],
            &["where", "a", "==", "uint:18446744073709551616"],
            &["sort-by"],
            &["sort-by", "--frame", "--frame", "a"],
            &["sort-by", "a", "b"],
            &["sort-by", "--reverse", "--reverse", "a"],
            &["sort-by", "--bogus", "a"],
            &["take"],
            &["take", "-1"],
            &["take", "+1"],
            &["take", "1.5"],
            &["take", ""],
            &["take", "1", "2"],
            &["take", "--frame", "--frame", "1"],
            &["take", "1", "--frame"],
            &["take", "99999999999999999999999999"],
            &["to"],
            &["to", "yaml"],
            &["to", "json", "--bogus"],
            &["to", "text", "--field"],
            &["from"],
            &["from", "text"],
            &["from", "json", "--bogus"],
            &["frob"],
        ];
        for case in bad {
            assert!(parse_stage(&argv(case)).is_err(), "should reject {:?}", case);
        }
    }

    #[test]
    fn where_literal_typing() {
        let lit = |s: &str| match parse_stage(&argv(&["where", "f", "==", s])).unwrap() {
            Stage::Where { literal, .. } => literal,
            _ => unreachable!(),
        };
        assert_eq!(lit("18446744073709551615"), Value::UInt(u64::MAX));
        assert_eq!(lit("9223372036854775807"), Value::Int(i64::MAX));
        assert_eq!(lit("-9223372036854775808"), Value::Int(i64::MIN));
        assert_eq!(lit("uint:5"), Value::UInt(5));
        assert_eq!(lit("int:-5"), Value::Int(-5));
        assert_eq!(lit("1.5"), Value::Float(1.5));
        assert_eq!(lit("1.0"), Value::Float(1.0));
        assert_eq!(lit("null"), Value::Null);
        assert_eq!(lit("true"), Value::Bool(true));
        assert_eq!(lit("\"int:5\""), Value::String("int:5".into()));
    }

    #[test]
    fn decoder_preserves_numeric_and_null_distinctions() {
        let v = decode_json(
            br#"{"a":18446744073709551615,"b":-9223372036854775808,"c":1.0,"d":1,"e":null,"f":0,"g":1e2,"h":9223372036854775808}"#,
        )
        .unwrap();
        assert_eq!(v.get_field("a"), Some(&Value::UInt(u64::MAX)));
        assert_eq!(v.get_field("b"), Some(&Value::Int(i64::MIN)));
        assert_eq!(v.get_field("c"), Some(&Value::Float(1.0)));
        assert_eq!(v.get_field("d"), Some(&Value::Int(1)));
        assert_eq!(v.get_field("e"), Some(&Value::Null));
        assert_eq!(v.get_field("g"), Some(&Value::Float(100.0)));
        assert_eq!(v.get_field("h"), Some(&Value::UInt(9223372036854775808)));
        assert_ne!(v.get_field("e"), v.get_field("f"));
        assert!(!filter_record(&v, "e", CompareOp::Equal, &Value::Int(0)).unwrap());
        assert!(filter_record(&v, "e", CompareOp::NotEqual, &Value::Int(0)).unwrap());
        assert_eq!(decode_json(b"  \n[1, 2 ]\t\r\n").unwrap(), Value::List(vec![Value::Int(1), Value::Int(2)]));
        assert_eq!(
            decode_json(br#""\u00e9\ud83d\ude00\n""#).unwrap(),
            Value::String("\u{e9}\u{1f600}\n".into())
        );
    }

    #[test]
    fn strict_decoder_rejects() {
        let mut deep_ok = String::new();
        // a 17-bracket nest puts an empty list at depth 16 (accepted); one more is rejected
        for _ in 0..17 {
            deep_ok.push('[');
        }
        for _ in 0..17 {
            deep_ok.push(']');
        }
        assert!(decode_json(deep_ok.as_bytes()).is_ok());
        let too_deep = alloc::format!("[{}]", deep_ok);
        assert_eq!(decode_json(too_deep.as_bytes()).unwrap_err(), PipelineError::MaxDepthExceeded);

        let mut wide = String::from("{");
        for i in 0..129 {
            if i > 0 {
                wide.push(',');
            }
            wide.push_str(&alloc::format!("\"k{}\":0", i));
        }
        wide.push('}');
        assert_eq!(decode_json(wide.as_bytes()).unwrap_err(), PipelineError::TooManyFields);
        let long_name = alloc::format!("{{\"{}\":1}}", "k".repeat(129));
        assert_eq!(decode_json(long_name.as_bytes()).unwrap_err(), PipelineError::FieldNameTooLong);

        assert_eq!(
            decode_json(br#"{"a":1,"a":2}"#).unwrap_err(),
            PipelineError::DuplicateField("a".into())
        );
        assert!(matches!(decode_json(b"18446744073709551616"), Err(PipelineError::PrecisionLoss(_))));
        assert!(matches!(decode_json(b"-9223372036854775809"), Err(PipelineError::PrecisionLoss(_))));
        assert_eq!(decode_json(b"1e999").unwrap_err(), PipelineError::NonFiniteFloat);
        let bad: &[&[u8]] = &[
            b"",
            b"   ",
            b"{",
            b"[1,]",
            b"[,1]",
            b"{\"a\":1,}",
            b"{a:1}",
            b"01",
            b"1.",
            b".5",
            b"+1",
            b"-",
            b"1e",
            b"NaN",
            b"Infinity",
            b"nul",
            b"'x'",
            b"\"abc",
            b"\"\\x\"",
            b"\"\\ud800\"",
            b"\"\\udc00\"",
            b"\"\\ud800\\u0041\"",
            b"\"tab\there\"",
            b"\"\xff\"",
            b"1 2",
            b"{} x",
            b"[] []",
            b"\xef\xbb\xbf{}",
            b"[1] // c",
        ];
        for case in bad {
            assert!(decode_json(case).is_err(), "should reject {:?}", String::from_utf8_lossy(case));
        }
        let huge = vec![b' '; MAX_JSON_DOCUMENT_BYTES + 1];
        assert!(decode_json(&huge).is_err());
    }

    #[test]
    fn jsonl_framing_rules() {
        assert_eq!(
            decode_jsonl(b"{\"a\":1}\n{\"a\":2}").unwrap(),
            vec![rec(&[("a", Value::Int(1))]), rec(&[("a", Value::Int(2))])]
        );
        assert_eq!(
            decode_jsonl(b"{\"a\":1}\r\n{\"a\":2}\r\n").unwrap().len(),
            2
        );
        assert!(decode_jsonl(b"").unwrap().is_empty());
        // blank lines are errors; a CR left after the delimiter CR is JSON whitespace
        assert!(decode_jsonl(b"{}\n\n{}\n").is_err());
        assert!(decode_jsonl(b"\n").is_err());
        assert!(decode_jsonl(b"{}\r\r\n").is_ok());
        assert!(decode_jsonl(b"{}\n{").is_err());
        assert!(decode_jsonl(b"{} {}\n").is_err());
        let err = decode_jsonl(b"{}\n{}\n[1,\n").unwrap_err();
        assert!(alloc::format!("{}", err).contains("line 3"));
        let mut long = Vec::from(&b"\""[..]);
        long.extend(core::iter::repeat(b'a').take(MAX_RECORD_LINE_BYTES));
        long.push(b'"');
        long.push(b'\n');
        assert_eq!(decode_jsonl(&long).unwrap_err(), PipelineError::RecordTooLarge);
        let mut ok = Vec::from(&b"\""[..]);
        ok.extend(core::iter::repeat(b'a').take(MAX_RECORD_LINE_BYTES - 2));
        ok.push(b'"');
        ok.push(b'\n');
        assert_eq!(decode_jsonl(&ok).unwrap().len(), 1);
    }

    fn sample_typed_row() -> Value {
        rec(&[
            ("pid", Value::UInt(u64::MAX)),
            ("neg", Value::Int(i64::MIN)),
            ("f", Value::Float(1.0)),
            ("n", Value::Null),
            ("b", Value::Bool(true)),
            ("s", Value::String("h\u{e9}llo \"q\" \n".into())),
            ("raw", Value::Bytes(vec![0, 0xff, 0x80])),
            ("path", Value::Path(vec![b'/', 0xff, 0xfe, b'x'])),
            ("ts", Value::Timestamp(-5)),
            ("dur", Value::Duration(i64::MAX)),
            ("list", Value::List(vec![Value::UInt(1), Value::Null, Value::List(vec![])])),
            ("nested", rec(&[("a", Value::Float(-0.5))])),
        ])
    }

    #[test]
    fn typed_roundtrip_preserves_every_variant() {
        let row = sample_typed_row();
        let mut out = Vec::new();
        encode_row(&Format::JsonlTyped, &row, true, &mut out).unwrap();
        encode_row(&Format::JsonlTyped, &row, false, &mut out).unwrap();
        assert_eq!(out.iter().filter(|&&b| b == b'\n').count(), 2);
        let rows = decode_jsonl_typed(&out).unwrap();
        assert_eq!(rows, vec![row.clone(), row.clone()]);

        let mut arr = Vec::new();
        encode_row(&Format::JsonTyped, &row, true, &mut arr).unwrap();
        encode_row(&Format::JsonTyped, &row, false, &mut arr).unwrap();
        finish_encoding(&Format::JsonTyped, true, &mut arr);
        assert_eq!(decode_rows(&Format::JsonTyped, &arr).unwrap(), vec![row.clone(), row.clone()]);

        let mut empty = Vec::new();
        finish_encoding(&Format::JsonTyped, false, &mut empty);
        assert!(decode_rows(&Format::JsonTyped, &empty).unwrap().is_empty());
        let mut empty = Vec::new();
        finish_encoding(&Format::Json, false, &mut empty);
        assert_eq!(empty, b"[]\n");
    }

    #[test]
    fn typed_depth_budget_matches_value_depth() {
        let mut v = Value::List(vec![]);
        for _ in 0..MAX_DEPTH {
            v = Value::List(vec![v]);
        }
        assert!(v.validate().is_ok());
        let mut out = Vec::new();
        encode_row(&Format::JsonlTyped, &v, true, &mut out).unwrap();
        assert_eq!(decode_jsonl_typed(&out).unwrap(), vec![v.clone()]);
        let deeper = Value::List(vec![v]);
        assert!(encode_row(&Format::JsonlTyped, &deeper, true, &mut Vec::new()).is_err());
    }

    #[test]
    fn typed_decoder_is_strict_and_plain_decoder_is_not_tagged() {
        let bad: &[&[u8]] = &[
            br#"{"a":1}"#,
            br#"{"cellbox":2,"value":["null",null]}"#,
            br#"{"cellbox":1}"#,
            br#"{"cellbox":1,"value":["null",null],"x":1}"#,
            br#"{"cellbox":1,"value":["wat",null]}"#,
            br#"{"cellbox":1,"value":["null",0]}"#,
            br#"{"cellbox":1,"value":["int",5]}"#,
            br#"{"cellbox":1,"value":["int","05"]}"#,
            br#"{"cellbox":1,"value":["int","-0"]}"#,
            br#"{"cellbox":1,"value":["int","9223372036854775808"]}"#,
            br#"{"cellbox":1,"value":["uint","-1"]}"#,
            br#"{"cellbox":1,"value":["bytes","abc"]}"#,
            br#"{"cellbox":1,"value":["bytes","AB"]}"#,
            br#"{"cellbox":1,"value":["path","00"]}"#,
            br#"{"cellbox":1,"value":["record",[["a",["null",null]],["a",["null",null]]]]}"#,
            br#"{"cellbox":1,"value":["record",[["a"]]]}"#,
            br#"{"cellbox":1,"value":["list",["null"]]}"#,
            br#"{"cellbox":1,"value":["float","1"]}"#,
            br#"{"cellbox":1,"value":["float",9007199254740993]}"#,
            br#"{"cellbox":1,"value":["null",null]} x"#,
        ];
        for case in bad {
            assert!(decode_json_typed(case).is_err(), "should reject {:?}", String::from_utf8_lossy(case));
        }
        // ordinary decoding never interprets an envelope
        let plain = decode_json(br#"{"cellbox":1,"value":["uint","5"]}"#).unwrap();
        assert!(matches!(plain, Value::Record(_)));
        assert_eq!(decode_json_typed(br#"{"value":["uint","5"],"cellbox":1}"#).unwrap(), Value::UInt(5));
        assert_eq!(decode_json_typed(br#"{"cellbox":1,"value":["float",2]}"#).unwrap(), Value::Float(2.0));
    }

    #[test]
    fn ordinary_encoding_refuses_lossy_values_and_keeps_float_identity() {
        let big = rec(&[("n", Value::UInt(u64::MAX))]);
        assert!(matches!(
            encode_row(&Format::Jsonl, &big, true, &mut Vec::new()),
            Err(PipelineError::PrecisionLoss(_))
        ));
        let mut out = Vec::new();
        encode_row(&Format::JsonlTyped, &big, true, &mut out).unwrap();
        assert!(String::from_utf8(out).unwrap().contains("[\"uint\",\"18446744073709551615\"]"));
        let mut keep = Vec::new();
        let p = rec(&[("p", Value::Path(vec![1]))]);
        assert!(encode_row(&Format::Json, &p, true, &mut keep).is_err());
        assert!(keep.is_empty(), "failed encode leaves out unchanged");

        let mut out = Vec::new();
        let row = rec(&[("f", Value::Float(1.0)), ("i", Value::Int(1)), ("s", Value::Float(1e300)), ("t", Value::Float(-0.0))]);
        encode_row(&Format::Jsonl, &row, true, &mut out).unwrap();
        assert_eq!(decode_jsonl(&out).unwrap(), vec![row]);

        let mut arr = Vec::new();
        encode_row(&Format::Json, &rec(&[("a", Value::Int(1))]), true, &mut arr).unwrap();
        encode_row(&Format::Json, &rec(&[("a", Value::Int(2))]), false, &mut arr).unwrap();
        finish_encoding(&Format::Json, true, &mut arr);
        assert_eq!(arr, b"[{\"a\":1},{\"a\":2}]\n");
        assert_eq!(decode_rows(&Format::Json, &arr).unwrap().len(), 2);
    }

    #[test]
    fn text_encoding_is_exact_and_refuses_tables() {
        let mut out = Vec::new();
        encode_row(&Format::Text, &Value::Bytes(vec![0xff, 0x00]), true, &mut out).unwrap();
        encode_row(&Format::Text, &Value::String("a b".into()), false, &mut out).unwrap();
        encode_row(&Format::Text, &Value::UInt(7), false, &mut out).unwrap();
        assert_eq!(out, b"\xff\x00\na b\n7\n");
        let r = rec(&[("name", Value::String("init".into())), ("pid", Value::UInt(1))]);
        let mut out = Vec::new();
        assert!(encode_row(&Format::Text, &r, true, &mut out).is_err());
        encode_row(&Format::TextField("pid".into()), &r, true, &mut out).unwrap();
        assert_eq!(out, b"1\n");
        assert!(encode_row(&Format::TextField("zz".into()), &r, true, &mut out).is_err());
        assert_eq!(out, b"1\n");
        assert!(encode_row(&Format::Text, &Value::List(vec![]), true, &mut out).is_err());
    }

    #[test]
    fn take_closes_upstream_without_extra_pull() {
        let (src, pulled, closed) = Counting::new(None);
        let mut t = apply_row_stage(Box::new(src), &Stage::Take { n: 2, frame: false }).unwrap();
        assert_eq!(pulled.get(), 0);
        assert!(t.next_row().unwrap().is_some());
        assert_eq!(closed.get(), 0);
        assert!(t.next_row().unwrap().is_some());
        assert_eq!(closed.get(), 1, "closed as soon as the nth row is delivered");
        assert!(t.next_row().unwrap().is_none());
        assert!(t.next_row().unwrap().is_none());
        t.close();
        assert_eq!(pulled.get(), 2);
        assert_eq!(closed.get(), 1, "close is not repeated upstream");
    }

    #[test]
    fn take_zero_closes_immediately_without_pulling() {
        let (src, pulled, closed) = Counting::new(None);
        let mut t = apply_row_stage(Box::new(src), &Stage::Take { n: 0, frame: false }).unwrap();
        assert_eq!(closed.get(), 1);
        assert!(t.next_row().unwrap().is_none());
        assert_eq!(pulled.get(), 0);
    }

    #[test]
    fn consumer_close_propagates_through_chain() {
        let (src, pulled, closed) = Counting::new(None);
        let mut s: Box<dyn RowSource> = Box::new(src);
        s = apply_row_stage(s, &parse_stage(&argv(&["where", "n", ">=", "uint:1"])).unwrap()).unwrap();
        s = apply_row_stage(s, &parse_stage(&argv(&["select", "n"])).unwrap()).unwrap();
        assert!(s.next_row().unwrap().is_some());
        assert_eq!(pulled.get(), 2);
        s.close();
        s.close();
        assert_eq!(closed.get(), 1);
        assert!(s.next_row().unwrap().is_none());
        assert_eq!(pulled.get(), 2);

        // where stopped by an outer take: producer is not pulled again after the last row
        let (src, pulled, closed) = Counting::new(None);
        let mut s: Box<dyn RowSource> = Box::new(src);
        s = apply_row_stage(s, &parse_stage(&argv(&["where", "n", "!=", "uint:1"])).unwrap()).unwrap();
        s = apply_row_stage(s, &Stage::Take { n: 2, frame: false }).unwrap();
        let rows = drain(s).unwrap();
        assert_eq!(rows, vec![rec(&[("n", Value::UInt(0))]), rec(&[("n", Value::UInt(2))])]);
        assert_eq!(pulled.get(), 3);
        assert_eq!(closed.get(), 1);
    }

    #[test]
    fn select_projects_in_argument_order_and_errors_close_upstream() {
        let rows = vec![
            rec(&[("a", Value::Int(1)), ("b", Value::Null), ("c", Value::Int(3))]),
            rec(&[("a", Value::Int(4)), ("b", Value::Null), ("c", Value::Int(6))]),
        ];
        let s = apply_row_stage(Box::new(VecSource::new(rows)), &Stage::Select(argv(&["c", "b"]))).unwrap();
        assert_eq!(
            drain(s).unwrap(),
            vec![
                rec(&[("c", Value::Int(3)), ("b", Value::Null)]),
                rec(&[("c", Value::Int(6)), ("b", Value::Null)])
            ]
        );

        let (src, pulled, closed) = Counting::new(None);
        let mut s = apply_row_stage(Box::new(src), &Stage::Select(argv(&["missing"]))).unwrap();
        assert_eq!(s.next_row().unwrap_err(), PipelineError::FieldNotFound("missing".into()));
        assert_eq!(closed.get(), 1);
        assert!(s.next_row().unwrap().is_none());
        assert_eq!(pulled.get(), 1);

        let mut s = apply_row_stage(Box::new(VecSource::new(vec![Value::Int(1)])), &Stage::Select(argv(&["a"]))).unwrap();
        assert_eq!(s.next_row().unwrap_err(), PipelineError::TypeMismatch);
    }

    #[test]
    fn where_error_aborts_and_closes() {
        let (src, _p, closed) = Counting::new(None);
        let st = parse_stage(&argv(&["where", "n", "==", "\"x\""])).unwrap();
        let mut s = apply_row_stage(Box::new(src), &st).unwrap();
        assert_eq!(s.next_row().unwrap_err(), PipelineError::TypeMismatch);
        assert_eq!(closed.get(), 1);
        assert!(s.next_row().unwrap().is_none());
    }

    #[test]
    fn sort_is_stable_null_first_and_reversible() {
        let rows = vec![
            rec(&[("k", Value::Int(2)), ("i", Value::Int(0))]),
            rec(&[("k", Value::Null), ("i", Value::Int(1))]),
            rec(&[("k", Value::UInt(1)), ("i", Value::Int(2))]),
            rec(&[("k", Value::Int(2)), ("i", Value::Int(3))]),
            rec(&[("k", Value::Null), ("i", Value::Int(4))]),
        ];
        let asc = apply_row_stage(
            Box::new(VecSource::new(rows.clone())),
            &parse_stage(&argv(&["sort-by", "k"])).unwrap(),
        )
        .unwrap();
        let order: Vec<i64> = drain(asc)
            .unwrap()
            .iter()
            .map(|r| match r.get_field("i") {
                Some(Value::Int(i)) => *i,
                _ => panic!(),
            })
            .collect();
        assert_eq!(order, vec![1, 4, 2, 0, 3]);
        let desc = apply_row_stage(
            Box::new(VecSource::new(rows)),
            &parse_stage(&argv(&["sort-by", "--reverse", "k"])).unwrap(),
        )
        .unwrap();
        let order: Vec<i64> = drain(desc)
            .unwrap()
            .iter()
            .map(|r| match r.get_field("i") {
                Some(Value::Int(i)) => *i,
                _ => panic!(),
            })
            .collect();
        assert_eq!(order, vec![0, 3, 2, 1, 4]);
    }

    #[test]
    fn sort_rejects_heterogeneous_and_missing_keys_before_emitting() {
        let mixed = vec![
            rec(&[("k", Value::Int(1))]),
            rec(&[("k", Value::String("a".into()))]),
        ];
        let mut s = apply_row_stage(Box::new(VecSource::new(mixed)), &Stage::SortBy { field: "k".into(), reverse: false, frame: false }).unwrap();
        assert_eq!(s.next_row().unwrap_err(), PipelineError::TypeMismatch);
        assert!(s.next_row().unwrap().is_none());
        let missing = vec![rec(&[("k", Value::Int(1))]), rec(&[("z", Value::Int(1))])];
        let mut s = apply_row_stage(Box::new(VecSource::new(missing)), &Stage::SortBy { field: "k".into(), reverse: false, frame: false }).unwrap();
        assert_eq!(s.next_row().unwrap_err(), PipelineError::FieldNotFound("k".into()));
    }

    #[test]
    fn sort_caps_row_count_and_payload_and_never_waits_on_endless_input() {
        let stage = Stage::SortBy { field: "n".into(), reverse: true, frame: false };
        let (src, pulled, closed) = Counting::new(None);
        let mut s = apply_row_stage(Box::new(src), &stage).unwrap();
        assert_eq!(s.next_row().unwrap_err(), PipelineError::SortLimitExceeded);
        assert_eq!(pulled.get(), MAX_SORT_RECORDS as u64 + 1, "stops pulling right after the cap");
        assert_eq!(closed.get(), 1);
        assert!(s.next_row().unwrap().is_none());

        // exactly MAX rows is fine, MAX + 1 is not
        let (src, _p, _c) = Counting::new(Some(MAX_SORT_RECORDS as u64));
        let rows = drain(apply_row_stage(Box::new(src), &stage).unwrap()).unwrap();
        assert_eq!(rows.len(), MAX_SORT_RECORDS);
        assert_eq!(rows[0].get_field("n"), Some(&Value::UInt(MAX_SORT_RECORDS as u64 - 1)));
        let (src, _p, closed) = Counting::new(Some(MAX_SORT_RECORDS as u64 + 1));
        assert_eq!(
            drain(apply_row_stage(Box::new(src), &stage).unwrap()).unwrap_err(),
            PipelineError::SortLimitExceeded
        );
        assert_eq!(closed.get(), 1);

        // payload cap
        let big: Vec<Value> = (0..5)
            .map(|i| rec(&[("n", Value::UInt(i)), ("pad", Value::String("x".repeat(1024 * 1024)))]))
            .collect();
        assert_eq!(
            drain(apply_row_stage(Box::new(VecSource::new(big)), &stage).unwrap()).unwrap_err(),
            PipelineError::SortLimitExceeded
        );
    }

    #[test]
    fn row_stage_refuses_byte_conversions() {
        let (src, _p, closed) = Counting::new(None);
        assert!(apply_row_stage(Box::new(src), &Stage::To(Format::Json)).is_err());
        assert_eq!(closed.get(), 1);
    }

    #[test]
    fn stage_sequence_admission() {
        let sel = Stage::Select(argv(&["a"]));
        let to = Stage::To(Format::JsonlTyped);
        let from = Stage::From(Format::Jsonl);
        assert_eq!(validate_stage_sequence(Input::Rows, &[]).unwrap(), SinkKind::Rows);
        assert_eq!(validate_stage_sequence(Input::Bytes, &[]).unwrap(), SinkKind::Bytes);
        assert_eq!(validate_stage_sequence(Input::Rows, &[sel.clone(), Stage::Take { n: 1, frame: false }]).unwrap(), SinkKind::Rows);
        assert_eq!(validate_stage_sequence(Input::Rows, &[sel.clone(), to.clone()]).unwrap(), SinkKind::Bytes);
        assert_eq!(validate_stage_sequence(Input::Bytes, &[from.clone(), sel.clone(), to.clone()]).unwrap(), SinkKind::Bytes);
        assert_eq!(validate_stage_sequence(Input::Bytes, &[from.clone()]).unwrap(), SinkKind::Rows);
        // bytes into a row stage without from
        assert!(validate_stage_sequence(Input::Bytes, &[sel.clone()]).is_err());
        // from not first, not after bytes, or after rows
        assert!(validate_stage_sequence(Input::Rows, &[from.clone()]).is_err());
        assert!(validate_stage_sequence(Input::Bytes, &[from.clone(), from.clone()]).is_err());
        assert!(validate_stage_sequence(Input::Bytes, &[Stage::From(Format::Text)]).is_err());
        // to: terminal, needs rows
        assert!(validate_stage_sequence(Input::Rows, &[to.clone(), sel.clone()]).is_err());
        assert!(validate_stage_sequence(Input::Rows, &[to.clone(), to.clone()]).is_err());
        assert!(validate_stage_sequence(Input::Bytes, &[to.clone()]).is_err());
        // stage-count bound
        let sixteen = vec![Stage::Take { n: 1, frame: false }; MAX_STAGES];
        assert!(validate_stage_sequence(Input::Rows, &sixteen).is_ok());
        let seventeen = vec![Stage::Take { n: 1, frame: false }; MAX_STAGES + 1];
        assert!(validate_stage_sequence(Input::Rows, &seventeen).is_err());
    }
}

#[cfg(test)]
mod frame_tests {
    use super::*;
    use alloc::rc::Rc;
    use alloc::vec;
    use core::cell::Cell;

    fn rec(fields: &[(&str, Value)]) -> Value {
        Value::Record(fields.iter().map(|(k, v)| (k.to_string(), v.clone())).collect())
    }

    fn argv(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    fn stage(words: &[&str]) -> Stage {
        parse_stage(&argv(words)).unwrap()
    }

    fn begin(sample_id: u64) -> Item {
        Item::Begin(FrameBegin { sample_id, monotonic_ns: sample_id * 10, meta: None })
    }

    fn end(complete: bool, dropped: u64) -> Item {
        Item::End(FrameEnd { complete, unavailable_fields: Vec::new(), dropped })
    }

    fn row(n: u64) -> Item {
        Item::Row(rec(&[("n", Value::UInt(n))]))
    }

    fn items(src: &mut dyn RowSource) -> Result<Vec<Item>, PipelineError> {
        let mut out = Vec::new();
        while let Some(item) = src.next_item()? {
            out.push(item);
        }
        Ok(out)
    }

    /// Endless frame producer: frame `f` has `rows` rows whose `n` counts down, so a sort
    /// changes their order. Counts every item pulled and every close.
    struct Frames {
        rows: u64,
        sample: u64,
        pos: u64,
        pulled: Rc<Cell<u64>>,
        closed: Rc<Cell<u32>>,
    }

    fn frames(rows: u64) -> (Box<dyn RowSource>, Rc<Cell<u64>>, Rc<Cell<u32>>) {
        let pulled = Rc::new(Cell::new(0));
        let closed = Rc::new(Cell::new(0));
        let src = Frames { rows, sample: 0, pos: 0, pulled: pulled.clone(), closed: closed.clone() };
        (Box::new(src), pulled, closed)
    }

    impl RowSource for Frames {
        fn next_item(&mut self) -> Result<Option<Item>, PipelineError> {
            if self.closed.get() > 0 {
                return Ok(None);
            }
            self.pulled.set(self.pulled.get() + 1);
            let pos = self.pos;
            self.pos = if pos == self.rows + 1 { 0 } else { pos + 1 };
            Ok(Some(match pos {
                0 => {
                    self.sample += 1;
                    begin(self.sample)
                }
                p if p <= self.rows => row(self.rows - p),
                _ => end(true, 0),
            }))
        }

        fn close(&mut self) {
            self.closed.set(self.closed.get() + 1);
        }
    }

    #[test]
    fn parse_accepts_frame_flags_and_refuses_duplicates() {
        assert_eq!(stage(&["take", "--frame", "3"]), Stage::Take { n: 3, frame: true });
        assert_eq!(stage(&["take", "3"]), Stage::Take { n: 3, frame: false });
        assert_eq!(
            stage(&["sort-by", "--frame", "--reverse", "cpu"]),
            Stage::SortBy { field: "cpu".into(), reverse: true, frame: true }
        );
        assert_eq!(
            stage(&["sort-by", "cpu", "--frame"]),
            Stage::SortBy { field: "cpu".into(), reverse: false, frame: true }
        );
        for bad in [
            &["take", "--frame", "--frame", "1"][..],
            &["take", "--frame"],
            &["take", "--frame", "x"],
            &["sort-by", "--frame", "--frame", "a"],
            &["sort-by", "--frame"],
        ] {
            assert!(matches!(parse_stage(&argv(bad)), Err(PipelineError::Usage(_))), "{:?}", bad);
        }
    }

    #[test]
    fn admission_separates_rows_and_frames() {
        let sel = stage(&["select", "n"]);
        let wh = stage(&["where", "n", ">", "uint:0"]);
        let take = stage(&["take", "1"]);
        let take_f = stage(&["take", "--frame", "1"]);
        let sort = stage(&["sort-by", "n"]);
        let sort_f = stage(&["sort-by", "--frame", "n"]);
        let jsonl = Stage::To(Format::Jsonl);
        let jsonl_t = Stage::To(Format::JsonlTyped);
        let ok = |input, stages: &[Stage]| validate_stage_sequence(input, stages).unwrap();
        let refused = |input, stages: &[Stage]| match validate_stage_sequence(input, stages) {
            Err(PipelineError::Sequence(msg)) => msg,
            other => panic!("expected refusal, got {:?}", other),
        };
        // Frames pass through select/where/--frame stages and leave Frames (or bytes after jsonl).
        assert_eq!(ok(Input::Frames, &[]), SinkKind::Frames);
        assert_eq!(ok(Input::Frames, &[sel.clone(), wh.clone(), sort_f.clone(), take_f.clone()]), SinkKind::Frames);
        assert_eq!(ok(Input::Frames, &[take_f.clone(), jsonl.clone()]), SinkKind::Bytes);
        assert_eq!(ok(Input::Frames, &[jsonl_t.clone()]), SinkKind::Bytes);
        // Rows keep the Phase 03 rules.
        assert_eq!(ok(Input::Rows, &[sel.clone(), take.clone(), sort.clone()]), SinkKind::Rows);
        // Rows-only stages on Frames name the --frame form.
        assert!(refused(Input::Frames, &[take.clone()]).contains("take --frame"));
        assert!(refused(Input::Frames, &[sel.clone(), sort.clone()]).contains("sort-by --frame"));
        // --frame on Rows is refused.
        refused(Input::Rows, &[take_f.clone()]);
        refused(Input::Rows, &[sort_f.clone()]);
        // Only jsonl encodes Frames, even when bounded by take --frame.
        for format in [Format::Json, Format::JsonTyped, Format::Text, Format::TextField("n".into())] {
            refused(Input::Frames, &[Stage::To(format.clone())]);
            refused(Input::Frames, &[take_f.clone(), Stage::To(format)]);
        }
        // from never yields Frames and needs a byte head.
        refused(Input::Frames, &[Stage::From(Format::Jsonl)]);
        assert_eq!(ok(Input::Bytes, &[Stage::From(Format::JsonlTyped), sel.clone()]), SinkKind::Rows);
        refused(Input::Bytes, &[Stage::From(Format::Jsonl), take_f]);
    }

    #[test]
    fn take_frame_emits_whole_frames_then_closes_upstream() {
        let (src, pulled, closed) = frames(2);
        let mut t = apply_row_stage(src, &stage(&["take", "--frame", "2"])).unwrap();
        let got = items(&mut *t).unwrap();
        assert_eq!(got, vec![begin(1), row(1), row(0), end(true, 0), begin(2), row(1), row(0), end(true, 0)]);
        assert_eq!(pulled.get(), 8, "nothing pulled after the 2nd End");
        assert_eq!(closed.get(), 1, "closed right after the 2nd End");

        let (src, pulled, closed) = frames(2);
        let mut t = apply_row_stage(src, &stage(&["take", "--frame", "0"])).unwrap();
        assert!(t.next_item().unwrap().is_none());
        assert_eq!((pulled.get(), closed.get()), (0, 1));
    }

    #[test]
    fn sort_frame_sorts_each_frame_and_never_reads_past_end() {
        let (src, pulled, _closed) = frames(3);
        let mut s = apply_row_stage(src, &stage(&["sort-by", "--frame", "n"])).unwrap();
        assert_eq!(s.next_item().unwrap(), Some(begin(1)));
        assert_eq!(pulled.get(), 1, "Begin is emitted as received");
        assert_eq!(s.next_item().unwrap(), Some(row(0)));
        assert_eq!(pulled.get(), 5, "rows buffered up to the End only");
        assert_eq!(s.next_item().unwrap(), Some(row(1)));
        assert_eq!(s.next_item().unwrap(), Some(row(2)));
        assert_eq!(s.next_item().unwrap(), Some(end(true, 0)));
        assert_eq!(pulled.get(), 5, "nothing pulled past End before the frame is emitted");
        assert_eq!(s.next_item().unwrap(), Some(begin(2)));
        assert_eq!(s.next_item().unwrap(), Some(row(0)));

        // Empty frames pass through.
        let src = VecSource::from_items(vec![begin(1), end(false, 3)]);
        let mut s = apply_row_stage(Box::new(src), &stage(&["sort-by", "--frame", "n"])).unwrap();
        assert_eq!(items(&mut *s).unwrap(), vec![begin(1), end(false, 3)]);
    }

    #[test]
    fn sort_frame_bounds_each_frame() {
        let max = MAX_SORT_RECORDS as u64;
        let (src, _p, _c) = frames(max);
        let mut s = apply_row_stage(src, &stage(&["sort-by", "--frame", "n"])).unwrap();
        let mut count = 0;
        while let Some(item) = s.next_item().unwrap() {
            if matches!(item, Item::End(_)) {
                break;
            }
            count += 1;
        }
        assert_eq!(count, MAX_SORT_RECORDS + 1, "Begin + 4096 rows");

        let (src, _p, closed) = frames(max + 1);
        let mut s = apply_row_stage(src, &stage(&["sort-by", "--frame", "n"])).unwrap();
        assert_eq!(s.next_item().unwrap(), Some(begin(1)));
        assert_eq!(s.next_item().unwrap_err(), PipelineError::SortLimitExceeded);
        assert_eq!(closed.get(), 1);
        assert!(s.next_item().unwrap().is_none());

        let pad = "x".repeat(1024 * 1024);
        let mut big = vec![begin(1)];
        big.extend((0..5).map(|i| Item::Row(rec(&[("n", Value::UInt(i)), ("pad", Value::String(pad.clone()))]))));
        big.push(end(true, 0));
        let mut s = apply_row_stage(Box::new(VecSource::from_items(big)), &stage(&["sort-by", "--frame", "n"])).unwrap();
        assert_eq!(s.next_item().unwrap(), Some(begin(1)));
        assert_eq!(s.next_item().unwrap_err(), PipelineError::SortLimitExceeded);
    }

    #[test]
    fn where_and_select_pass_markers_unchanged() {
        let meta = rec(&[("schema", Value::String("t.v1".into()))]);
        let b = Item::Begin(FrameBegin { sample_id: 7, monotonic_ns: 9, meta: Some(meta) });
        let e = Item::End(FrameEnd { complete: false, unavailable_fields: vec!["cpu".into()], dropped: 4 });
        let src = VecSource::from_items(vec![
            b.clone(),
            Item::Row(rec(&[("n", Value::UInt(0)), ("m", Value::UInt(5))])),
            Item::Row(rec(&[("n", Value::UInt(2)), ("m", Value::UInt(6))])),
            e.clone(),
        ]);
        let mut s = apply_row_stage(Box::new(src), &stage(&["where", "n", ">", "uint:0"])).unwrap();
        s = apply_row_stage(s, &stage(&["select", "m"])).unwrap();
        assert_eq!(items(&mut *s).unwrap(), vec![b, Item::Row(rec(&[("m", Value::UInt(6))])), e]);
    }

    fn encode(format: &Format, item: Item) -> Result<String, PipelineError> {
        let mut out = Vec::new();
        encode_item(format, Category::Frames, item, true, &mut out)?;
        Ok(String::from_utf8(out).unwrap())
    }

    #[test]
    fn frame_envelopes_encode_exactly_and_round_trip_typed() {
        let meta = rec(&[("schema", Value::String("top.cell.v1".into()))]);
        let b = Item::Begin(FrameBegin { sample_id: 3, monotonic_ns: 42, meta: Some(meta.clone()) });
        assert_eq!(
            encode(&Format::Jsonl, b.clone()).unwrap(),
            "{\"frame\":\"begin\",\"sample_id\":3,\"monotonic_ns\":42,\"meta\":{\"schema\":\"top.cell.v1\"}}\n"
        );
        assert_eq!(encode(&Format::Jsonl, begin(1)).unwrap(), "{\"frame\":\"begin\",\"sample_id\":1,\"monotonic_ns\":10}\n");
        assert_eq!(encode(&Format::Jsonl, row(5)).unwrap(), "{\"frame\":\"row\",\"value\":{\"n\":5}}\n");
        let e = Item::End(FrameEnd { complete: false, unavailable_fields: vec!["cpu_tenths".into()], dropped: 2 });
        assert_eq!(
            encode(&Format::Jsonl, e.clone()).unwrap(),
            "{\"frame\":\"end\",\"complete\":false,\"unavailable_fields\":[\"cpu_tenths\"],\"dropped\":2}\n"
        );

        // Typed envelopes are ordinary typed records.
        let typed = encode(&Format::JsonlTyped, b).unwrap();
        let decoded = decode_jsonl_typed(typed.as_bytes()).unwrap();
        assert_eq!(
            decoded,
            vec![rec(&[
                ("frame", Value::String("begin".into())),
                ("sample_id", Value::UInt(3)),
                ("monotonic_ns", Value::UInt(42)),
                ("meta", meta),
            ])]
        );
        let typed = encode(&Format::JsonlTyped, e).unwrap();
        assert_eq!(decode_jsonl_typed(typed.as_bytes()).unwrap()[0].get_field("dropped"), Some(&Value::UInt(2)));

        // Untyped JSON refuses inexact counters; typed keeps them.
        let huge = Item::Begin(FrameBegin { sample_id: 1, monotonic_ns: u64::MAX, meta: None });
        assert!(matches!(encode(&Format::Jsonl, huge.clone()), Err(PipelineError::PrecisionLoss(_))));
        assert!(encode(&Format::JsonlTyped, huge).is_ok());

        // Frames never encode as json/text, and a marker is not a row.
        assert!(encode(&Format::Json, row(1)).is_err());
        assert!(encode(&Format::Text, row(1)).is_err());
        let mut out = Vec::new();
        assert!(encode_item(&Format::Jsonl, Category::Rows, begin(1), true, &mut out).is_err());
        assert!(out.is_empty());
        encode_item(&Format::Jsonl, Category::Rows, row(1), true, &mut out).unwrap();
        assert_eq!(out, b"{\"n\":1}\n");
    }

    #[test]
    fn envelope_counts_toward_depth_bound() {
        let mut deep = Value::UInt(1);
        for _ in 0..MAX_DEPTH {
            deep = Value::List(vec![deep]);
        }
        // The bare value is at the bound; wrapped in a row envelope it is one level deeper.
        assert!(deep.validate().is_ok());
        assert_eq!(encode(&Format::Jsonl, Item::Row(deep)).unwrap_err(), PipelineError::MaxDepthExceeded);
    }

    fn guarded(category: Category, list: Vec<Item>) -> Result<Vec<Item>, PipelineError> {
        let mut g = check_items(Box::new(VecSource::from_items(list)), category);
        let result = items(&mut *g);
        if result.is_err() {
            assert!(g.next_item().unwrap().is_none(), "a failed guard stays at end");
        }
        result
    }

    #[test]
    fn order_guard_accepts_valid_streams_and_refuses_every_violation() {
        let ok = vec![begin(1), row(1), end(true, 0), begin(4), end(true, 0)];
        assert_eq!(guarded(Category::Frames, ok.clone()).unwrap(), ok);
        assert_eq!(guarded(Category::Rows, vec![row(1), row(2)]).unwrap().len(), 2);
        let bad: [(Category, Vec<Item>); 7] = [
            (Category::Rows, vec![row(1), begin(1)]),
            (Category::Rows, vec![end(true, 0)]),
            (Category::Frames, vec![row(1)]),
            (Category::Frames, vec![begin(1), begin(2)]),
            (Category::Frames, vec![end(true, 0)]),
            (Category::Frames, vec![begin(1), row(1)]),
            (Category::Frames, vec![begin(2), end(true, 0), begin(2), end(true, 0)]),
        ];
        for (category, list) in bad {
            assert!(
                matches!(guarded(category, list.clone()), Err(PipelineError::Sequence(_))),
                "{:?} {:?}",
                category,
                list
            );
        }
    }

    #[test]
    fn order_guard_closes_the_producer_on_violation() {
        struct Bad {
            closed: Rc<Cell<u32>>,
        }
        impl RowSource for Bad {
            fn next_item(&mut self) -> Result<Option<Item>, PipelineError> {
                Ok(Some(Item::Row(Value::Null)))
            }
            fn close(&mut self) {
                self.closed.set(self.closed.get() + 1);
            }
        }
        let closed = Rc::new(Cell::new(0));
        let mut g = check_items(Box::new(Bad { closed: closed.clone() }), Category::Frames);
        assert!(g.next_item().is_err());
        assert_eq!(closed.get(), 1);
    }
}

#[cfg(test)]
mod hook_tests {
    use super::*;
    use alloc::vec;

    // ─── InterruptLatch ─────────────────────────────────────────────────────

    /// A latch whose poll counts calls and reports an interrupt from poll number `hit_at`.
    fn counted(hit_at: Option<u32>) -> (Rc<InterruptLatch>, Rc<Cell<u32>>) {
        let polls = Rc::new(Cell::new(0u32));
        let seen = polls.clone();
        let latch = InterruptLatch::new(Box::new(move || {
            seen.set(seen.get() + 1);
            hit_at.is_some_and(|n| seen.get() >= n)
        }));
        (latch, polls)
    }

    #[test]
    fn latch_check_polls_exactly_once_per_sixteen_calls() {
        let (latch, polls) = counted(None);
        for call in 1..=100u32 {
            assert!(!latch.check());
            assert_eq!(polls.get(), call / 16, "after call {}", call);
        }
        // The third real poll (call 48) observes the interrupt; calls before it do not.
        let (latch, polls) = counted(Some(3));
        for call in 1..48u32 {
            assert!(!latch.check(), "call {}", call);
        }
        assert!(latch.check());
        assert_eq!(polls.get(), 3);
    }

    #[test]
    fn latch_poll_now_polls_every_time_until_set() {
        let (latch, polls) = counted(Some(6));
        for call in 1..=5u32 {
            assert!(!latch.poll_now());
            assert_eq!(polls.get(), call);
        }
        assert!(!latch.is_set());
        assert!(latch.poll_now());
        assert_eq!(polls.get(), 6);
    }

    #[test]
    fn latch_is_sticky_and_stops_polling_once_set() {
        let (latch, polls) = counted(Some(1));
        assert!(latch.poll_now());
        for _ in 0..64 {
            assert!(latch.is_set());
            assert!(latch.check());
            assert!(latch.poll_now());
        }
        assert_eq!(polls.get(), 1);

        // An explicit `set` latches without a poll, and checks then skip polling.
        let (latch, polls) = counted(None);
        assert!(!latch.is_set());
        latch.set();
        for _ in 0..64 {
            assert!(latch.check());
            assert!(latch.poll_now());
        }
        assert_eq!(polls.get(), 0);
    }

    #[test]
    fn inert_latch_never_reports_an_interrupt_until_set() {
        let latch = InterruptLatch::inert();
        for _ in 0..1000 {
            assert!(!latch.check());
        }
        for _ in 0..10 {
            assert!(!latch.poll_now());
        }
        assert!(!latch.is_set());
        latch.set();
        assert!(latch.is_set() && latch.check() && latch.poll_now());
    }

    // ─── Completion forwarding ──────────────────────────────────────────────

    const CODE: i32 = 3;

    /// Producer reporting `CODE` once closed; records `close`, `completion` reads and drop.
    struct Fake {
        items: VecDeque<Item>,
        fail: bool,
        closed: bool,
        log: Rc<RefCell<Vec<&'static str>>>,
    }

    impl RowSource for Fake {
        fn next_item(&mut self) -> Result<Option<Item>, PipelineError> {
            if self.closed {
                return Ok(None);
            }
            if let Some(item) = self.items.pop_front() {
                return Ok(Some(item));
            }
            if self.fail {
                self.fail = false;
                return Err(PipelineError::Source("boom".to_string()));
            }
            Ok(None)
        }

        fn close(&mut self) {
            self.closed = true;
            self.log.borrow_mut().push("close");
        }

        fn completion(&self) -> Option<i32> {
            self.log.borrow_mut().push("completion");
            self.closed.then_some(CODE)
        }
    }

    impl Drop for Fake {
        fn drop(&mut self) {
            self.log.borrow_mut().push("drop");
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq)]
    enum Ending {
        /// Drain to `Ok(None)`.
        Eof,
        /// The producer fails after its items; drain to the error.
        Error,
        /// Close the chain before any pull.
        CloseNow,
        /// Close the chain after one pull.
        CloseAfterOne,
    }

    fn row(n: u64) -> Item {
        Item::Row(Value::Record(vec![("n".to_string(), Value::UInt(n))]))
    }

    fn begin(sample_id: u64) -> Item {
        Item::Begin(FrameBegin { sample_id, monotonic_ns: sample_id, meta: None })
    }

    fn end() -> Item {
        Item::End(FrameEnd { complete: true, unavailable_fields: Vec::new(), dropped: 0 })
    }

    /// Build `stage(check_items(fake))`, end it per `ending`, and assert the chain reports the
    /// producer's completion, read after the producer's close and before its drop.
    fn assert_forwards(stage: Option<&Stage>, category: Category, items: Vec<Item>, ending: Ending) {
        let log = Rc::new(RefCell::new(Vec::new()));
        let fake = Fake {
            items: VecDeque::from(items),
            fail: ending == Ending::Error,
            closed: false,
            log: log.clone(),
        };
        let guarded = check_items(Box::new(fake), category);
        let mut chain = match stage {
            Some(stage) => apply_row_stage(guarded, stage).unwrap(),
            None => guarded,
        };
        match ending {
            Ending::Eof => while chain.next_item().unwrap().is_some() {},
            Ending::Error => loop {
                match chain.next_item() {
                    Ok(Some(_)) => {}
                    Ok(None) => panic!("{:?}: stream ended without the producer error", stage),
                    Err(e) => {
                        assert_eq!(e, PipelineError::Source("boom".to_string()));
                        break;
                    }
                }
            },
            Ending::CloseNow => chain.close(),
            Ending::CloseAfterOne => {
                chain.next_item().unwrap();
                chain.close();
            }
        }
        assert_eq!(chain.completion(), Some(CODE), "{:?} {:?}", stage, ending);
        assert_eq!(*log.borrow(), ["close", "completion", "drop"], "{:?} {:?}", stage, ending);
    }

    #[test]
    fn every_row_stage_forwards_the_producer_completion() {
        let rows = || vec![row(2), row(1), row(3)];
        let stages = [
            None,
            Some(Stage::Select(vec!["n".to_string()])),
            Some(Stage::Where {
                field: "n".to_string(),
                op: CompareOp::GreaterOrEqual,
                literal: Value::UInt(0),
            }),
            Some(Stage::Take { n: 5, frame: false }),
            Some(Stage::SortBy { field: "n".to_string(), reverse: false, frame: false }),
        ];
        for stage in &stages {
            for ending in [Ending::Eof, Ending::Error, Ending::CloseNow, Ending::CloseAfterOne] {
                assert_forwards(stage.as_ref(), Category::Rows, rows(), ending);
            }
        }
    }

    #[test]
    fn take_forwards_the_completion_when_it_closes_upstream_itself() {
        // `take 2` of 3 rows closes the producer after the 2nd row; `take 0` before any pull.
        for n in [2, 0] {
            let stage = Stage::Take { n, frame: false };
            for ending in [Ending::Eof, Ending::CloseNow] {
                assert_forwards(Some(&stage), Category::Rows, vec![row(1), row(2), row(3)], ending);
            }
        }
    }

    #[test]
    fn frame_sort_forwards_the_producer_completion() {
        let frames = || vec![begin(1), row(2), row(1), end(), begin(2), row(5), end()];
        let stage = Stage::SortBy { field: "n".to_string(), reverse: true, frame: true };
        for ending in [Ending::Eof, Ending::Error, Ending::CloseNow, Ending::CloseAfterOne] {
            assert_forwards(Some(&stage), Category::Frames, frames(), ending);
        }
    }

    #[test]
    fn stage_failure_forwards_the_producer_completion() {
        // `select` over a non-record row fails the stage, which closes the producer.
        let log = Rc::new(RefCell::new(Vec::new()));
        let fake = Fake {
            items: VecDeque::from(vec![Item::Row(Value::Null)]),
            fail: false,
            closed: false,
            log: log.clone(),
        };
        let guarded = check_items(Box::new(fake), Category::Rows);
        let mut chain = apply_row_stage(guarded, &Stage::Select(vec!["n".to_string()])).unwrap();
        assert_eq!(chain.next_item().unwrap_err(), PipelineError::TypeMismatch);
        assert_eq!(chain.completion(), Some(CODE));
        assert_eq!(*log.borrow(), ["close", "completion", "drop"]);
    }

    #[test]
    fn completion_is_live_while_held_and_none_by_default() {
        struct Live;
        impl RowSource for Live {
            fn next_item(&mut self) -> Result<Option<Item>, PipelineError> {
                Ok(None)
            }
            fn close(&mut self) {}
            fn completion(&self) -> Option<i32> {
                Some(7)
            }
        }
        let select = Stage::Select(vec!["n".to_string()]);
        let chain = apply_row_stage(check_items(Box::new(Live), Category::Rows), &select).unwrap();
        assert_eq!(chain.completion(), Some(7));

        let mut chain =
            apply_row_stage(check_items(Box::new(VecSource::new(vec![])), Category::Rows), &select).unwrap();
        assert!(chain.next_item().unwrap().is_none());
        assert_eq!(chain.completion(), None);
    }

    // ─── typed_json_len ─────────────────────────────────────────────────────

    fn assert_len_matches(v: &Value) {
        let expected = value_to_json(v, true).map(|s| s.len());
        assert_eq!(typed_json_len(v), expected, "{:?}", v);
    }

    #[test]
    fn typed_json_len_equals_the_encoded_length() {
        let escapes = "q\"b\\n\nr\rt\tb\x08f\x0c nul\0 us\x1f del\x7f c1\u{85} é 漢 😀";
        let scalars = vec![
            Value::Null,
            Value::Bool(true),
            Value::Bool(false),
            Value::Int(0),
            Value::Int(i64::MIN),
            Value::Int(i64::MAX),
            Value::UInt(u64::MAX),
            Value::Float(0.0),
            Value::Float(-0.0),
            Value::Float(1.5e-7),
            Value::Float(1e300),
            Value::Float(f64::MIN_POSITIVE),
            Value::Float(-123.25),
            Value::String(String::new()),
            Value::String(escapes.to_string()),
            Value::Bytes(Vec::new()),
            Value::Bytes((0..=255).collect()),
            Value::Path(b"/a/\xff\0b\n".to_vec()),
            Value::Timestamp(i64::MIN),
            Value::Timestamp(1_700_000_000_000_000_000),
            Value::Duration(-5),
        ];
        for v in &scalars {
            assert_len_matches(v);
        }
        let record = Value::Record(vec![
            (escapes.to_string(), Value::List(scalars.clone())),
            ("nested".to_string(), Value::List(vec![Value::List(vec![]), Value::Record(vec![])])),
            ("null".to_string(), Value::Null),
        ]);
        assert_len_matches(&record);
        assert_len_matches(&Value::List(vec![record.clone(), record]));
    }

    #[test]
    fn typed_json_len_reports_the_same_errors() {
        let empty = typed_json_len(&Value::String(String::new())).unwrap();
        assert_eq!(empty, value_to_json(&Value::String(String::new()), true).unwrap().len());
        // Exactly at the line bound, and one byte over it.
        let at = Value::String("a".repeat(MAX_RECORD_LINE_BYTES - empty));
        assert_eq!(typed_json_len(&at), Ok(MAX_RECORD_LINE_BYTES));
        assert_len_matches(&at);
        let over = Value::String("a".repeat(MAX_RECORD_LINE_BYTES - empty + 1));
        assert_eq!(typed_json_len(&over), Err(PipelineError::RecordTooLarge));
        assert_len_matches(&over);
        // Escapes count at their encoded width: 6 bytes per control char.
        let controls = Value::String("\u{1}".repeat((MAX_RECORD_LINE_BYTES - empty) / 6 + 1));
        assert_eq!(typed_json_len(&controls), Err(PipelineError::RecordTooLarge));
        assert_len_matches(&controls);
        let bytes = Value::Bytes(vec![0xab; MAX_RECORD_LINE_BYTES / 2]);
        assert_eq!(typed_json_len(&bytes), Err(PipelineError::RecordTooLarge));
        assert_len_matches(&bytes);
        // Non-finite floats fail, also after an oversized prefix (the same first error).
        for f in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(typed_json_len(&Value::Float(f)), Err(PipelineError::NonFiniteFloat));
            assert_len_matches(&Value::Float(f));
            let late = Value::List(vec![over.clone(), Value::Float(f)]);
            assert_eq!(typed_json_len(&late), Err(PipelineError::NonFiniteFloat));
            assert_len_matches(&late);
        }
    }
}
