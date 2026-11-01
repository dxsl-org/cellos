// SPDX-License-Identifier: MPL-2.0
//! Bounded SMBIOS reader for the system identity (structure type 1).
//!
//! Reads one entry point (64-bit `_SM3_` preferred, then 32-bit `_SM_`), maps
//! the structure table it names, and returns the first type 1 structure's
//! manufacturer, product name and UUID.
//!
//! Contract:
//! - Never panics. Every malformed, truncated, oversized or unmappable input
//!   yields `None`; a caller treats `None` exactly like firmware without SMBIOS.
//! - Every firmware range is mapped through the caller's mapper before it is
//!   read, so SMBIOS memory gets the same admission as ACPI tables.
//! - Strings are firmware-controlled. They are truncated to
//!   [`SMBIOS_STRING_MAX`] bytes, kept as raw bytes, and grant no authority.

/// Longest string prefix kept from one SMBIOS string.
pub const SMBIOS_STRING_MAX: usize = 64;

/// Upper bound on a structure table this reader maps. The 32-bit entry point
/// cannot describe more than 64 KiB; a 64-bit entry point declaring more than
/// this is refused rather than mapped.
pub const SMBIOS_TABLE_MAX: usize = 0x10_0000;

const EP32_ANCHOR: &[u8; 4] = b"_SM_";
const EP32_DMI_ANCHOR: &[u8; 5] = b"_DMI_";
/// The 32-bit entry point is 0x1F bytes; SMBIOS 2.1 published 0x1E in error,
/// so 0x1E..=0x20 is accepted while 0x1F bytes are always read.
const EP32_READ_LEN: usize = 0x1F;
const EP32_DECLARED: core::ops::RangeInclusive<usize> = 0x1E..=0x20;

const EP64_ANCHOR: &[u8; 5] = b"_SM3_";
const EP64_READ_LEN: usize = 0x18;
const EP64_DECLARED: core::ops::RangeInclusive<usize> = 0x18..=0x20;
const EP64_REVISION: u8 = 1;

const TYPE_SYSTEM: u8 = 1;
const TYPE_END_OF_TABLE: u8 = 127;
const STRUCTURE_HEADER_LEN: usize = 4;
/// Type 1 through the serial-number string index (SMBIOS 2.0).
const SYSTEM_MIN_LEN: usize = 0x08;
/// Type 1 through the UUID (SMBIOS 2.1+).
const SYSTEM_UUID_END: usize = 0x18;

/// A string copied out of firmware memory, truncated to
/// [`SMBIOS_STRING_MAX`] bytes. String index 0 ("none") is the empty string.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SmbiosString {
    bytes: [u8; SMBIOS_STRING_MAX],
    len: u8,
}

impl SmbiosString {
    /// The string a structure names with index 0.
    pub const EMPTY: Self = Self {
        bytes: [0; SMBIOS_STRING_MAX],
        len: 0,
    };

    /// Copies at most [`SMBIOS_STRING_MAX`] bytes of `source`.
    pub fn new(source: &[u8]) -> Self {
        let len = source.len().min(SMBIOS_STRING_MAX);
        let mut bytes = [0; SMBIOS_STRING_MAX];
        bytes[..len].copy_from_slice(&source[..len]);
        Self {
            bytes,
            len: len as u8,
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len as usize]
    }
}

/// System identity from SMBIOS structure type 1.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SmbiosSystem {
    pub manufacturer: SmbiosString,
    pub product: SmbiosString,
    /// RFC 4122 byte order. `None` when the structure predates SMBIOS 2.1 or
    /// firmware reports the all-zero ("not present") or all-ones ("not set")
    /// value.
    pub uuid: Option<[u8; 16]>,
}

/// Where an entry point says the structure table lives.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct StructureTable {
    phys: usize,
    len: usize,
    /// The 32-bit entry point also bounds the structure count.
    max_structures: Option<u16>,
    version: (u8, u8),
}

/// Reads the system identity from the SMBIOS entry points firmware reported.
///
/// `entry_32` and `entry_64` are physical entry-point addresses (`None` when
/// absent). A valid 64-bit entry point is used; otherwise the 32-bit one.
/// `map_physical` must make `[phys, phys + len)` readable and return its
/// virtual base, or `None` to refuse the range.
pub fn discover(
    entry_32: Option<usize>,
    entry_64: Option<usize>,
    mut map_physical: impl FnMut(usize, usize) -> Option<usize>,
) -> Option<SmbiosSystem> {
    let table = entry_64
        .and_then(|phys| {
            read_entry_point(phys, EP64_READ_LEN, 6, &mut map_physical, parse_entry_64)
        })
        .or_else(|| {
            entry_32.and_then(|phys| {
                read_entry_point(phys, EP32_READ_LEN, 5, &mut map_physical, parse_entry_32)
            })
        })?;
    table.phys.checked_add(table.len)?;
    let virt = map_physical(table.phys, table.len)?;
    // SAFETY: the mapper made `[virt, virt + len)` readable; boot runs on one
    // CPU, so firmware memory does not change while the slice is alive.
    let bytes = unsafe { core::slice::from_raw_parts(virt as *const u8, table.len) };
    find_system(bytes, table.max_structures, table.version)
}

/// Maps `read_len` bytes at `phys`, then the full declared length the byte at
/// `length_offset` names (never less than `read_len`), and hands the mapped
/// bytes to `parse`.
fn read_entry_point(
    phys: usize,
    read_len: usize,
    length_offset: usize,
    map_physical: &mut impl FnMut(usize, usize) -> Option<usize>,
    parse: fn(&[u8]) -> Option<StructureTable>,
) -> Option<StructureTable> {
    if phys == 0 {
        return None;
    }
    phys.checked_add(read_len)?;
    let virt = map_physical(phys, read_len)?;
    // SAFETY: the mapper made `[virt, virt + read_len)` readable and
    // `length_offset < read_len`.
    let declared = unsafe { core::ptr::read_volatile((virt + length_offset) as *const u8) };
    let len = read_len.max(declared as usize);
    phys.checked_add(len)?;
    let virt = map_physical(phys, len)?;
    // SAFETY: the mapper made `[virt, virt + len)` readable; single-CPU boot.
    let bytes = unsafe { core::slice::from_raw_parts(virt as *const u8, len) };
    parse(bytes)
}

fn checksum_ok(bytes: &[u8]) -> bool {
    bytes.iter().fold(0u8, |sum, byte| sum.wrapping_add(*byte)) == 0
}

fn le_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

fn le_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn le_u64(bytes: &[u8], offset: usize) -> Option<u64> {
    Some(u64::from_le_bytes(
        bytes.get(offset..offset + 8)?.try_into().ok()?,
    ))
}

/// SMBIOS 2.x entry point: `_SM_` with its checksum over the declared length,
/// then the `_DMI_` intermediate record with its own checksum over 0x10..0x1F.
fn parse_entry_32(ep: &[u8]) -> Option<StructureTable> {
    if ep.get(..4)? != EP32_ANCHOR.as_slice() {
        return None;
    }
    let declared = *ep.get(5)? as usize;
    if !EP32_DECLARED.contains(&declared) || !checksum_ok(ep.get(..declared)?) {
        return None;
    }
    if ep.get(0x10..0x15)? != EP32_DMI_ANCHOR.as_slice() || !checksum_ok(ep.get(0x10..0x1F)?) {
        return None;
    }
    let len = le_u16(ep, 0x16)? as usize;
    let phys = le_u32(ep, 0x18)? as usize;
    let count = le_u16(ep, 0x1C)?;
    if len < STRUCTURE_HEADER_LEN || phys == 0 || count == 0 {
        return None;
    }
    Some(StructureTable {
        phys,
        len,
        max_structures: Some(count),
        version: (ep[6], ep[7]),
    })
}

/// SMBIOS 3.x entry point: `_SM3_`, checksum over the declared length, entry
/// point revision 1, and a structure table bounded by [`SMBIOS_TABLE_MAX`].
fn parse_entry_64(ep: &[u8]) -> Option<StructureTable> {
    if ep.get(..5)? != EP64_ANCHOR.as_slice() {
        return None;
    }
    let declared = *ep.get(6)? as usize;
    if !EP64_DECLARED.contains(&declared) || !checksum_ok(ep.get(..declared)?) {
        return None;
    }
    if *ep.get(0x0A)? != EP64_REVISION {
        return None;
    }
    let len = usize::try_from(le_u32(ep, 0x0C)?).ok()?;
    let phys = usize::try_from(le_u64(ep, 0x10)?).ok()?;
    if !(STRUCTURE_HEADER_LEN..=SMBIOS_TABLE_MAX).contains(&len) || phys == 0 {
        return None;
    }
    Some(StructureTable {
        phys,
        len,
        max_structures: None,
        version: (ep[7], ep[8]),
    })
}

/// Walks the structure table to the first type 1 structure.
///
/// A structure whose formatted area or string set runs past the table, a
/// formatted length below the 4-byte header, the end-of-table marker or the
/// 32-bit structure count ends the walk with `None`.
fn find_system(
    table: &[u8],
    max_structures: Option<u16>,
    version: (u8, u8),
) -> Option<SmbiosSystem> {
    let mut offset = 0usize;
    let mut walked = 0u16;
    loop {
        if max_structures.is_some_and(|max| walked >= max) {
            return None;
        }
        let header = table.get(offset..offset.checked_add(STRUCTURE_HEADER_LEN)?)?;
        let kind = header[0];
        let formatted_len = header[1] as usize;
        if formatted_len < STRUCTURE_HEADER_LEN {
            return None;
        }
        let strings_start = offset + formatted_len;
        let formatted = table.get(offset..strings_start)?;
        let rest = table.get(strings_start..)?;
        // The string set ends at the first double NUL: strings are non-empty,
        // and a structure without strings carries just the two terminators.
        let strings_len = rest.windows(2).position(|pair| pair == [0, 0])? + 2;
        let strings = &rest[..strings_len];
        match kind {
            TYPE_SYSTEM => return system_from(formatted, strings, version),
            TYPE_END_OF_TABLE => return None,
            _ => {}
        }
        offset = strings_start + strings_len;
        walked = walked.saturating_add(1);
    }
}

fn system_from(formatted: &[u8], strings: &[u8], version: (u8, u8)) -> Option<SmbiosSystem> {
    if formatted.len() < SYSTEM_MIN_LEN {
        return None;
    }
    let manufacturer = string_at(strings, formatted[4])?;
    let product = string_at(strings, formatted[5])?;
    let uuid = formatted
        .get(0x08..SYSTEM_UUID_END)
        .and_then(|raw| uuid_from(raw.try_into().ok()?, version));
    Some(SmbiosSystem {
        manufacturer,
        product,
        uuid,
    })
}

/// Resolves a 1-based string index; 0 is the empty string, and an index past
/// the string set is malformed.
fn string_at(strings: &[u8], index: u8) -> Option<SmbiosString> {
    if index == 0 {
        return Some(SmbiosString::EMPTY);
    }
    strings
        .split(|byte| *byte == 0)
        .nth(index as usize - 1)
        .filter(|string| !string.is_empty())
        .map(SmbiosString::new)
}

/// SMBIOS 2.6+ encodes the first three UUID fields little-endian; earlier
/// versions are kept in wire order.
fn uuid_from(mut raw: [u8; 16], version: (u8, u8)) -> Option<[u8; 16]> {
    if raw.iter().all(|byte| *byte == 0) || raw.iter().all(|byte| *byte == 0xFF) {
        return None;
    }
    if version >= (2, 6) {
        raw[0..4].reverse();
        raw[4..6].reverse();
        raw[6..8].reverse();
    }
    Some(raw)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use alloc::vec::Vec;

    const UUID_WIRE: [u8; 16] = [
        0x33, 0x22, 0x11, 0x00, 0x55, 0x44, 0x77, 0x66, 0x88, 0x99, 0xAA, 0xBB, 0xCC, 0xDD, 0xEE,
        0xFF,
    ];
    const UUID_RFC4122: [u8; 16] = [
        0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB, 0xCC, 0xDD, 0xEE,
        0xFF,
    ];

    fn structure(kind: u8, formatted_tail: &[u8], strings: &[&[u8]]) -> Vec<u8> {
        let mut bytes = vec![kind, (4 + formatted_tail.len()) as u8, 0x00, 0x01];
        bytes.extend_from_slice(formatted_tail);
        if strings.is_empty() {
            bytes.push(0);
        }
        for string in strings {
            bytes.extend_from_slice(string);
            bytes.push(0);
        }
        bytes.push(0);
        bytes
    }

    fn system(uuid: [u8; 16], strings: &[&[u8]]) -> Vec<u8> {
        // manufacturer=1, product=2, version=0, serial=0, UUID, wake-up type.
        let mut tail = vec![1, 2, 0, 0];
        tail.extend_from_slice(&uuid);
        tail.push(6);
        structure(TYPE_SYSTEM, &tail, strings)
    }

    fn table() -> Vec<u8> {
        let mut table = structure(0, &[1, 2, 0, 0xE8, 0, 0], &[b"SeaBIOS", b"rel-1.16"]);
        table.extend(system(
            UUID_WIRE,
            &[b"QEMU", b"Standard PC (Q35 + ICH9, 2009)"],
        ));
        table.extend(structure(TYPE_END_OF_TABLE, &[], &[]));
        table
    }

    fn seal(ep: &mut [u8], checksum_at: usize, range: core::ops::Range<usize>) {
        ep[checksum_at] = 0;
        let sum = ep[range]
            .iter()
            .fold(0u8, |sum, byte| sum.wrapping_add(*byte));
        ep[checksum_at] = 0u8.wrapping_sub(sum);
    }

    fn entry_32(table_phys: u32, table_len: u16, count: u16) -> Vec<u8> {
        let mut ep = vec![0u8; 0x1F];
        ep[0..4].copy_from_slice(EP32_ANCHOR);
        ep[5] = 0x1F;
        ep[6] = 2;
        ep[7] = 8;
        ep[0x10..0x15].copy_from_slice(EP32_DMI_ANCHOR);
        ep[0x16..0x18].copy_from_slice(&table_len.to_le_bytes());
        ep[0x18..0x1C].copy_from_slice(&table_phys.to_le_bytes());
        ep[0x1C..0x1E].copy_from_slice(&count.to_le_bytes());
        seal(&mut ep, 0x15, 0x10..0x1F);
        seal(&mut ep, 4, 0..0x1F);
        ep
    }

    fn entry_64(table_phys: u64, table_len: u32) -> Vec<u8> {
        let mut ep = vec![0u8; 0x18];
        ep[0..5].copy_from_slice(EP64_ANCHOR);
        ep[6] = 0x18;
        ep[7] = 3;
        ep[8] = 0;
        ep[0x0A] = EP64_REVISION;
        ep[0x0C..0x10].copy_from_slice(&table_len.to_le_bytes());
        ep[0x10..0x18].copy_from_slice(&table_phys.to_le_bytes());
        seal(&mut ep, 5, 0..0x18);
        ep
    }

    const EP32_PHYS: usize = 0x100;
    const EP64_PHYS: usize = 0x200;
    const TABLE_PHYS: usize = 0x1000;

    /// Physical memory with the table at `TABLE_PHYS` and the given entry points.
    fn image(ep32: Option<&[u8]>, ep64: Option<&[u8]>, table: &[u8]) -> Vec<u8> {
        let mut memory = vec![0u8; TABLE_PHYS + table.len() + 0x100];
        if let Some(ep) = ep32 {
            memory[EP32_PHYS..EP32_PHYS + ep.len()].copy_from_slice(ep);
        }
        if let Some(ep) = ep64 {
            memory[EP64_PHYS..EP64_PHYS + ep.len()].copy_from_slice(ep);
        }
        memory[TABLE_PHYS..TABLE_PHYS + table.len()].copy_from_slice(table);
        memory
    }

    fn run(memory: &[u8], ep32: bool, ep64: bool) -> Option<SmbiosSystem> {
        let base = memory.as_ptr() as usize;
        discover(
            ep32.then_some(EP32_PHYS),
            ep64.then_some(EP64_PHYS),
            |phys: usize, len: usize| {
                (phys.checked_add(len)? <= memory.len()).then_some(base + phys)
            },
        )
    }

    fn expect_qemu(system: Option<SmbiosSystem>) {
        let system = system.expect("valid table");
        assert_eq!(system.manufacturer.as_bytes(), b"QEMU");
        assert_eq!(system.product.as_bytes(), b"Standard PC (Q35 + ICH9, 2009)");
        assert_eq!(system.uuid, Some(UUID_RFC4122));
    }

    #[test]
    fn reads_type_1_through_either_entry_point() {
        let table = table();
        let ep32 = entry_32(TABLE_PHYS as u32, table.len() as u16, 3);
        let ep64 = entry_64(TABLE_PHYS as u64, table.len() as u32);
        let memory = image(Some(&ep32), Some(&ep64), &table);
        expect_qemu(run(&memory, true, false));
        expect_qemu(run(&memory, false, true));
        expect_qemu(run(&memory, true, true));
        assert_eq!(run(&memory, false, false), None);
    }

    #[test]
    fn an_invalid_64_bit_entry_point_falls_back_to_the_32_bit_one() {
        let table = table();
        let ep32 = entry_32(TABLE_PHYS as u32, table.len() as u16, 3);
        let mut ep64 = entry_64(TABLE_PHYS as u64, table.len() as u32);
        ep64[0x10] ^= 1;
        let memory = image(Some(&ep32), Some(&ep64), &table);
        assert_eq!(run(&memory, false, true), None);
        expect_qemu(run(&memory, true, true));
    }

    #[test]
    fn bad_checksums_and_anchors_are_refused() {
        let table = table();
        let good32 = entry_32(TABLE_PHYS as u32, table.len() as u16, 3);

        let mut outer = good32.clone();
        outer[0x1E] ^= 1;
        let mut intermediate = good32.clone();
        intermediate[0x16] ^= 1;
        seal(&mut intermediate, 4, 0..0x1F);
        let mut anchor = good32.clone();
        anchor[0x10] = b'X';
        seal(&mut anchor, 4, 0..0x1F);
        for ep in [outer, intermediate, anchor] {
            assert_eq!(run(&image(Some(&ep), None, &table), true, false), None);
        }

        let mut ep64 = entry_64(TABLE_PHYS as u64, table.len() as u32);
        ep64[0x0C] ^= 1;
        assert_eq!(run(&image(None, Some(&ep64), &table), false, true), None);
        let mut revision = entry_64(TABLE_PHYS as u64, table.len() as u32);
        revision[0x0A] = 2;
        seal(&mut revision, 5, 0..0x18);
        assert_eq!(
            run(&image(None, Some(&revision), &table), false, true),
            None
        );
    }

    #[test]
    fn declared_entry_point_lengths_outside_the_accepted_range_are_refused() {
        let table = table();
        for declared in [0x00, 0x1D, 0x21, 0xFF] {
            let mut ep = entry_32(TABLE_PHYS as u32, table.len() as u16, 3);
            ep[5] = declared;
            seal(&mut ep, 4, 0..0x1F);
            assert_eq!(run(&image(Some(&ep), None, &table), true, false), None);
        }
        for declared in [0x00, 0x17, 0x21] {
            let mut ep = entry_64(TABLE_PHYS as u64, table.len() as u32);
            ep[6] = declared;
            seal(&mut ep, 5, 0..0x18);
            assert_eq!(run(&image(None, Some(&ep), &table), false, true), None);
        }
    }

    #[test]
    fn oversized_and_unmappable_tables_are_refused() {
        let table = table();
        let oversized = entry_64(TABLE_PHYS as u64, SMBIOS_TABLE_MAX as u32 + 1);
        assert_eq!(
            run(&image(None, Some(&oversized), &table), false, true),
            None
        );

        // A declared length past mapped memory is refused by the mapper.
        let past_end = entry_64(TABLE_PHYS as u64, 0x10_0000);
        assert_eq!(
            run(&image(None, Some(&past_end), &table), false, true),
            None
        );

        let overflowing = entry_64(u64::MAX - 8, 0x100);
        assert_eq!(
            run(&image(None, Some(&overflowing), &table), false, true),
            None
        );

        let empty = entry_32(TABLE_PHYS as u32, 0, 3);
        assert_eq!(run(&image(Some(&empty), None, &table), true, false), None);
    }

    #[test]
    fn truncated_tables_never_read_past_their_declared_length() {
        let table = table();
        let system_end = table.len() - structure(TYPE_END_OF_TABLE, &[], &[]).len();
        // Every cut before type 1's string set terminates is refused, whatever
        // lies after the cut in memory.
        for cut in 0..system_end {
            let ep = entry_64(TABLE_PHYS as u64, cut as u32);
            let found = run(&image(None, Some(&ep), &table), false, true);
            assert_eq!(found, None, "cut at {cut}");
        }
        let ep = entry_64(TABLE_PHYS as u64, system_end as u32);
        expect_qemu(run(&image(None, Some(&ep), &table), false, true));
    }

    #[test]
    fn the_32_bit_structure_count_bounds_the_walk() {
        let table = table();
        let ep = entry_32(TABLE_PHYS as u32, table.len() as u16, 1);
        assert_eq!(run(&image(Some(&ep), None, &table), true, false), None);
    }

    #[test]
    fn malformed_structures_end_the_walk() {
        // A formatted length below the header size.
        let mut short = table();
        short[1] = 3;
        assert_eq!(find_system(&short, None, (3, 0)), None);
        // End-of-table before type 1.
        let mut ended = structure(TYPE_END_OF_TABLE, &[], &[]);
        ended.extend(system(UUID_WIRE, &[b"QEMU", b"q35"]));
        assert_eq!(find_system(&ended, None, (3, 0)), None);
        // A string index past the string set.
        let one_string = system(UUID_WIRE, &[b"QEMU"]);
        assert_eq!(find_system(&one_string, None, (3, 0)), None);
        // A type 1 shorter than SMBIOS 2.0's fixed fields.
        let tiny = structure(TYPE_SYSTEM, &[1, 2], &[b"QEMU", b"q35"]);
        assert_eq!(find_system(&tiny, None, (3, 0)), None);
    }

    #[test]
    fn uuid_and_string_edge_cases() {
        // Pre-2.6 keeps wire order; absent / unset UUIDs are None.
        let wire = system(UUID_WIRE, &[b"QEMU", b"q35"]);
        assert_eq!(
            find_system(&wire, None, (2, 5)).unwrap().uuid,
            Some(UUID_WIRE)
        );
        let zero = system([0; 16], &[b"QEMU", b"q35"]);
        assert_eq!(find_system(&zero, None, (3, 0)).unwrap().uuid, None);
        let unset = system([0xFF; 16], &[b"QEMU", b"q35"]);
        assert_eq!(find_system(&unset, None, (3, 0)).unwrap().uuid, None);

        // SMBIOS 2.0 type 1 has no UUID.
        let v20 = structure(TYPE_SYSTEM, &[1, 2, 0, 0], &[b"OEM", b"Box"]);
        let found = find_system(&v20, None, (2, 0)).unwrap();
        assert_eq!(found.uuid, None);
        assert_eq!(found.product.as_bytes(), b"Box");

        // Index 0 is the empty string; long strings are truncated.
        let long = [b'A'; 200];
        let mut tail = vec![0, 1, 0, 0];
        tail.extend_from_slice(&UUID_WIRE);
        let indexed = structure(TYPE_SYSTEM, &tail, &[&long]);
        let found = find_system(&indexed, None, (3, 0)).unwrap();
        assert_eq!(found.manufacturer, SmbiosString::EMPTY);
        assert_eq!(found.product.as_bytes(), &long[..SMBIOS_STRING_MAX]);
    }
}
