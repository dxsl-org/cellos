//! HID report descriptor parser (HID 1.11 §6.2.2).
//!
//! Walks the item stream a HID device returns for `GET_DESCRIPTOR(REPORT)` and
//! builds a flat field map: which bit range of which report carries which
//! usage. This is what lets the driver support arbitrary HID devices instead of
//! only the fixed boot-protocol reports.
//!
//! Main items (`Input`/`Output`/`Feature`) consume the accumulated local usages
//! and emit a field spanning `report_count` entries of `report_size` bits each,
//! in declaration order within their `report_id`. Global items mutate state that
//! persists until changed (with `Push`/`Pop`); local items are cleared after
//! every main item, per spec.

extern crate alloc;

use alloc::vec::Vec;

/// The top-level collection a field belongs to.
///
/// A combo receiver exposes a keyboard interface and a mouse interface, and may
/// pack both into one report, so routing needs the collection, not just the
/// usage page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HidCollection {
    Other,
    Keyboard,
    Mouse,
    Consumer,
}

/// One decoded field declaration from the report descriptor.
///
/// A single `Input` main item can span several entries (a mouse's X/Y/Wheel, a
/// keyboard's six key slots), and its local usage list or range says what each
/// entry means. Both forms are common in the wild, so the field keeps the whole
/// list rather than collapsing to its first element.
#[derive(Clone, Debug)]
pub struct HidField {
    pub report_id: u8,
    /// Bit offset within the report payload (after any report-ID byte).
    pub offset_bits: u32,
    pub size_bits: u32,
    pub count: u32,
    /// Explicit usages from local `Usage` items, page in the high 16 bits.
    pub usages: Vec<u32>,
    /// Range start from `Usage Minimum` (page in the high 16 bits).
    pub usage_min: Option<u32>,
    /// Range end from `Usage Maximum` (page in the high 16 bits).
    pub usage_max: Option<u32>,
    /// `true` for `Variable` (one value per entry), `false` for `Array`
    /// (the entry carries the usage index itself — how keyboards report keys).
    pub is_variable: bool,
    pub logical_min: i32,
    pub logical_max: i32,
    pub collection: HidCollection,
}

impl HidField {
    /// Usage for entry `index` of a `Variable` field.
    ///
    /// Explicit usages win; otherwise the entry maps into the declared range.
    /// Returns `None` when the descriptor said nothing usable about this entry,
    /// so the caller drops it instead of inventing a usage.
    pub fn variable_usage(&self, index: u32) -> Option<u32> {
        if let Some(u) = self.usages.get(index as usize) {
            return Some(*u);
        }
        let (min, max) = (self.usage_min?, self.usage_max?);
        if index > max.saturating_sub(min) {
            return None;
        }
        Some(min.saturating_add(index))
    }

    /// Usage an `Array` entry's value denotes.
    ///
    /// Array values are indices into the declared usage range; a keyboard
    /// declares `Usage Minimum 0`, so its value is already the usage ID.
    pub fn array_usage(&self, value: u32) -> Option<u32> {
        match (self.usage_min, self.usage_max) {
            (Some(min), Some(max)) => {
                let base = min & 0xFFFF;
                let limit = max & 0xFFFF;
                if value >= base && value <= limit {
                    Some((min & 0xFFFF_0000) | value)
                } else if value <= limit.saturating_sub(base) {
                    Some((min & 0xFFFF_0000) | base.saturating_add(value))
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

/// Every field of one report ID.
#[derive(Clone, Debug)]
pub struct ReportLayout {
    pub report_id: u8,
    pub fields: Vec<HidField>,
    /// Total payload bits (excluding the report-ID byte).
    pub total_bits: u32,
}

/// Parse result for a whole report descriptor.
#[derive(Clone, Debug, Default)]
pub struct HidReportMap {
    pub reports: Vec<ReportLayout>,
    /// True when the descriptor declares any Report ID — reports then carry a
    /// leading report-ID byte that the caller must strip.
    pub uses_report_ids: bool,
}

impl HidReportMap {
    pub fn layout(&self, report_id: u8) -> Option<&ReportLayout> {
        self.reports.iter().find(|r| r.report_id == report_id)
    }

    /// The report ID a device sends when only one report exists.
    pub fn sole_report_id(&self) -> Option<u8> {
        if self.reports.len() == 1 {
            Some(self.reports[0].report_id)
        } else {
            None
        }
    }

    pub fn has_collection(&self, c: HidCollection) -> bool {
        self.reports
            .iter()
            .any(|r| r.fields.iter().any(|f| f.collection == c))
    }
}

/// Mutable global item state (HID 1.11 §6.2.2.8).
#[derive(Clone, Copy, Default)]
struct Globals {
    usage_page: u32,
    logical_min: i32,
    logical_max: i32,
    report_size: u32,
    report_count: u32,
    report_id: u8,
}

/// Local item state — cleared after every main item (§6.2.2.9).
#[derive(Default)]
struct Locals {
    usages: Vec<u32>,
    usage_min: Option<u32>,
    usage_max: Option<u32>,
}

/// Sign-extend an item's payload, which HID encodes as a signed integer.
fn sign_extend(raw: u32, size: usize) -> i32 {
    match size {
        1 => raw as u8 as i8 as i32,
        2 => raw as u16 as i16 as i32,
        _ => raw as i32,
    }
}

/// Sign-extend a logical bound, clamped to the field's own bit width.
///
/// A `Logical Minimum` of `0x8000` with `Report Size` 16 is −32768, but devices
/// also declare bounds wider than the field; clamping keeps the decode in range.
fn clamp_logical(value: i32, size_bits: u32) -> i32 {
    if size_bits == 0 || size_bits >= 32 {
        return value;
    }
    let half = 1i64 << (size_bits - 1);
    let max = half - 1;
    let min = -half;
    (value as i64).clamp(min, max) as i32
}

/// Parse a HID report descriptor into a field map.
///
/// Unknown items are skipped by size, which is what makes this tolerant of
/// vendor-specific descriptors: an unrecognised tag cannot desynchronise the
/// stream because the length prefix is always authoritative.
pub fn parse_report_descriptor(data: &[u8]) -> HidReportMap {
    let mut map = HidReportMap::default();
    let mut globals = Globals::default();
    let mut globals_stack: Vec<Globals> = Vec::new();
    let mut locals = Locals::default();
    // Collection stack: (usage_page, usage) of each open collection.
    let mut collections: Vec<(u32, u32)> = Vec::new();

    let mut i = 0usize;
    while i < data.len() {
        let prefix = data[i];
        i += 1;

        // Long item (0xFE): [0xFE][bDataSize][bLongItemTag][data…]
        if prefix == 0xFE {
            if i + 1 >= data.len() {
                break;
            }
            let dsize = data[i] as usize;
            i += 2;
            i = i.saturating_add(dsize).min(data.len());
            continue;
        }

        let size = match prefix & 0x03 {
            0 => 0usize,
            1 => 1,
            2 => 2,
            _ => 4,
        };
        let item_type = (prefix >> 2) & 0x03;
        let tag = (prefix >> 4) & 0x0F;

        if i + size > data.len() {
            break;
        }
        let mut raw: u32 = 0;
        for (k, byte) in data[i..i + size].iter().enumerate() {
            raw |= (*byte as u32) << (8 * k);
        }
        i += size;

        match item_type {
            // ── Main items ─────────────────────────────────────────────────
            0 => match tag {
                0x8 | 0x9 | 0xB => {
                    // Input / Output / Feature. Only Input carries device→host
                    // data; Output (LEDs) and Feature are not decoded.
                    if tag == 0x8 && globals.report_size > 0 && globals.report_count > 0 {
                        let flags = raw;
                        let is_variable = flags & 0x02 != 0;

                        let report_id = globals.report_id;
                        let offset = match map.reports.iter().find(|r| r.report_id == report_id) {
                            Some(r) => r.total_bits,
                            None => {
                                map.reports.push(ReportLayout {
                                    report_id,
                                    fields: Vec::new(),
                                    total_bits: 0,
                                });
                                0
                            }
                        };

                        let collection = classify_collection(&collections);
                        let field = HidField {
                            report_id,
                            offset_bits: offset,
                            size_bits: globals.report_size,
                            count: globals.report_count,
                            usages: locals.usages.clone(),
                            usage_min: locals.usage_min,
                            usage_max: locals.usage_max,
                            is_variable,
                            logical_min: globals.logical_min,
                            logical_max: globals.logical_max,
                            collection,
                        };

                        let bits = globals.report_size.saturating_mul(globals.report_count);
                        if let Some(r) = map.reports.iter_mut().find(|r| r.report_id == report_id) {
                            r.fields.push(field);
                            r.total_bits = r.total_bits.saturating_add(bits);
                        }
                    }
                    locals = Locals::default();
                }
                0xA => {
                    // Collection start
                    let page = locals
                        .usages
                        .first()
                        .map(|u| u >> 16)
                        .unwrap_or(globals.usage_page);
                    let usage = locals.usages.first().map(|u| u & 0xFFFF).unwrap_or(0);
                    collections.push((page, usage));
                    locals = Locals::default();
                }
                0xC => {
                    // End Collection
                    collections.pop();
                    locals = Locals::default();
                }
                _ => locals = Locals::default(),
            },

            // ── Global items ───────────────────────────────────────────────
            1 => match tag {
                0x0 => globals.usage_page = raw,
                0x1 => globals.logical_min = sign_extend(raw, size),
                0x2 => globals.logical_max = sign_extend(raw, size),
                0x7 => globals.report_size = raw,
                0x8 => {
                    globals.report_id = raw as u8;
                    if raw != 0 {
                        map.uses_report_ids = true;
                    }
                }
                0x9 => globals.report_count = raw,
                0xA => globals_stack.push(globals),
                0xB => {
                    if let Some(g) = globals_stack.pop() {
                        globals = g;
                    }
                }
                // Physical Min/Max, Unit, Unit Exponent: parsed but unused —
                // they describe scaling, not layout.
                0x3..=0x6 => {}
                _ => {}
            },

            // ── Local items ────────────────────────────────────────────────
            2 => match tag {
                0x0 => {
                    // A bare Usage after a Usage Page inherits that page unless
                    // it is a 32-bit extended usage (high half carries its own).
                    let u = if size == 4 {
                        raw
                    } else {
                        (globals.usage_page << 16) | raw
                    };
                    locals.usages.push(u);
                }
                0x1 => {
                    locals.usage_min = Some((globals.usage_page << 16) | (raw & 0xFFFF));
                }
                0x2 => {
                    locals.usage_max = Some((globals.usage_page << 16) | (raw & 0xFFFF));
                }
                // Designator / String index items do not affect the bit layout.
                _ => {}
            },

            // ── Reserved ───────────────────────────────────────────────────
            _ => {}
        }
    }

    // Logical bounds were parsed before `report_size` may have been finalised;
    // clamp them now against each field's own width.
    for r in map.reports.iter_mut() {
        for f in r.fields.iter_mut() {
            f.logical_min = clamp_logical(f.logical_min, f.size_bits);
            f.logical_max = clamp_logical(f.logical_max, f.size_bits);
        }
    }

    map
}

/// Classify the innermost recognised collection in the open stack.
fn classify_collection(open: &[(u32, u32)]) -> HidCollection {
    use crate::hid::keymap::{
        GD_KEYBOARD, GD_MOUSE, GD_POINTER, PAGE_CONSUMER, PAGE_GENERIC_DESKTOP,
    };
    for (page, usage) in open.iter().rev() {
        match (*page, *usage) {
            (PAGE_GENERIC_DESKTOP, GD_KEYBOARD) => return HidCollection::Keyboard,
            (PAGE_GENERIC_DESKTOP, GD_MOUSE) | (PAGE_GENERIC_DESKTOP, GD_POINTER) => {
                return HidCollection::Mouse
            }
            (PAGE_CONSUMER, _) => return HidCollection::Consumer,
            _ => {}
        }
    }
    HidCollection::Other
}
