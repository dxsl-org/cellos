//! HID input report decoder.
//!
//! Turns a raw report payload into `(usage_page, usage, value)` triples using
//! the layout [`super::report`] extracted from the report descriptor. Handles
//! both field kinds a real device mixes:
//!
//! * **Variable** fields — one value per entry (mouse X/Y/Wheel, button bits).
//! * **Array** fields — the entry carries the usage (a keyboard's key slots).
//!
//! Report IDs are stripped here, so callers pass the payload as the device sent
//! it and the decoder splits it by the layout's declarations.
//!
//! Note on "no change": an all-zero report is meaningful (every key released),
//! so the decoder reports every entry it can resolve and lets the caller diff
//! against its previous state. It never filters zeroes itself — a filter here
//! would swallow releases.

extern crate alloc;

use alloc::vec::Vec;

use super::report::{HidField, HidReportMap, ReportLayout};

/// One decoded entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HidValue {
    pub page: u32,
    pub usage: u32,
    /// Raw field value, sign-extended per the field's bit width.
    pub value: i32,
    pub collection: super::report::HidCollection,
    /// `true` when the entry came from a `Variable` field (a value), `false`
    /// when from an `Array` field (the entry *is* the usage). Modifier keys and
    /// mouse buttons are Variable; a keyboard's key slots are Array.
    pub is_variable: bool,
}

/// Extract a `size_bits`-wide little-endian bit field from `data`.
///
/// Returns `None` when the field runs past the payload, which is how a
/// truncated or mis-framed report is rejected instead of read out of bounds.
fn extract_bits(data: &[u8], offset_bits: u32, size_bits: u32) -> Option<u32> {
    if size_bits == 0 || size_bits > 32 {
        return None;
    }
    let end = offset_bits.checked_add(size_bits)?;
    if (end as usize).div_ceil(8) > data.len() {
        return None;
    }
    let mut acc: u64 = 0;
    for bit in 0..size_bits {
        let src = offset_bits + bit;
        let byte = data[(src / 8) as usize];
        if byte & (1 << (src % 8)) != 0 {
            acc |= 1u64 << bit;
        }
    }
    Some(acc as u32)
}

/// Sign-extend a raw field value using the field's declared logical range.
///
/// HID packs signed values two's-complement into `report_size` bits, so the
/// width — not the logical maximum — decides the sign bit.
fn to_signed(raw: u32, size_bits: u32) -> i32 {
    if size_bits == 0 || size_bits >= 32 {
        return raw as i32;
    }
    let sign_bit = 1u32 << (size_bits - 1);
    if raw & sign_bit != 0 {
        (raw as i64 - (1i64 << size_bits)) as i32
    } else {
        raw as i32
    }
}

/// Decode one report payload against its layout.
///
/// `report_id` selects the layout; pass the device's report-ID byte. For devices
/// that use a single unnumbered report, pass 0.
pub fn decode_report(map: &HidReportMap, report_id: u8, payload: &[u8]) -> Vec<HidValue> {
    let Some(layout) = map.layout(report_id) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for field in &layout.fields {
        decode_field(field, payload, &mut out);
    }
    out
}

/// Decode a report whose payload may or may not carry a leading report-ID byte.
///
/// HID devices that declare `Report ID` prefix every report with it; devices
/// that do not must not have one stripped. Getting this wrong shifts the whole
/// bit stream, so the decision is made from the parsed descriptor.
pub fn decode_auto(map: &HidReportMap, report: &[u8]) -> Vec<HidValue> {
    if map.uses_report_ids {
        let Some((&id, rest)) = report.split_first() else {
            return Vec::new();
        };
        decode_report(map, id, rest)
    } else {
        let id = map.sole_report_id().unwrap_or(0);
        decode_report(map, id, report)
    }
}

fn decode_field(field: &HidField, payload: &[u8], out: &mut Vec<HidValue>) {
    if field.is_variable {
        for index in 0..field.count {
            let offset = field.offset_bits + index * field.size_bits;
            let Some(raw) = extract_bits(payload, offset, field.size_bits) else {
                return;
            };
            let Some(usage) = field.variable_usage(index) else {
                continue;
            };
            out.push(HidValue {
                page: usage >> 16,
                usage: usage & 0xFFFF,
                value: to_signed(raw, field.size_bits),
                collection: field.collection,
                is_variable: true,
            });
        }
    } else {
        // Array field: each entry is a usage index. Duplicate entries are
        // dropped — a keyboard lists a key once even if it appears twice.
        let mut seen: Vec<u32> = Vec::new();
        for index in 0..field.count {
            let offset = field.offset_bits + index * field.size_bits;
            let Some(raw) = extract_bits(payload, offset, field.size_bits) else {
                return;
            };
            let Some(usage) = field.array_usage(raw) else {
                continue;
            };
            let usage_code = usage & 0xFFFF;
            if usage_code == 0 || seen.contains(&usage_code) {
                continue;
            }
            seen.push(usage_code);
            out.push(HidValue {
                page: usage >> 16,
                usage: usage_code,
                value: 1,
                collection: field.collection,
                is_variable: false,
            });
        }
    }
}

/// Bit offsets a keyboard array occupies, for callers wanting raw slot access.
pub fn array_slots(layout: &ReportLayout) -> impl Iterator<Item = HidField> + '_ {
    layout.fields.iter().filter(|f| !f.is_variable).cloned()
}
