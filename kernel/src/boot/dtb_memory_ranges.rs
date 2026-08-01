use super::{MapError, MemoryMapEntry, MemoryType};

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Range {
    pub start: usize,
    pub end: usize,
}

impl Range {
    pub fn ram(start: usize, size: usize) -> Result<Self, MapError> {
        let end = start.checked_add(size).ok_or(MapError::InvalidRange)?;
        let start = align_up(start, super::PAGE_SIZE)?;
        let end = end & !(super::PAGE_SIZE - 1);
        if start >= end {
            return Err(MapError::InvalidRange);
        }
        Ok(Self { start, end })
    }

    pub fn protected(start: usize, size: usize) -> Result<Self, MapError> {
        let end = start.checked_add(size).ok_or(MapError::InvalidRange)?;
        let start = start & !(super::PAGE_SIZE - 1);
        let end = align_up(end, super::PAGE_SIZE)?;
        if start >= end {
            return Err(MapError::InvalidRange);
        }
        Ok(Self { start, end })
    }

    pub fn overlaps(self, other: Self) -> bool {
        self.start < other.end && other.start < self.end
    }
}

pub(super) fn is_enabled(node: &fdt::node::FdtNode<'_, '_>) -> bool {
    node.property("status").is_none_or(|status| {
        let value = status.value.strip_suffix(&[0]).unwrap_or(status.value);
        matches!(value, b"ok" | b"okay")
    })
}

pub(super) fn align_up(value: usize, align: usize) -> Result<usize, MapError> {
    value
        .checked_add(align - 1)
        .map(|rounded| rounded & !(align - 1))
        .ok_or(MapError::InvalidRange)
}

pub(super) fn push_range(
    ranges: &mut [Range],
    count: &mut usize,
    range: Range,
) -> Result<(), MapError> {
    let slot = ranges.get_mut(*count).ok_or(MapError::TooManyRanges)?;
    *slot = range;
    *count += 1;
    Ok(())
}

pub(super) fn sort_ranges(ranges: &mut [Range]) {
    ranges.sort_unstable_by_key(|range| (range.start, range.end));
}

pub(super) fn merge_ranges(ranges: &mut [Range], count: usize) -> usize {
    let mut merged = 0;
    for index in 0..count {
        let range = ranges[index];
        if merged > 0 && range.start <= ranges[merged - 1].end {
            ranges[merged - 1].end = ranges[merged - 1].end.max(range.end);
        } else {
            ranges[merged] = range;
            merged += 1;
        }
    }
    merged
}

pub(super) fn add_boundaries(
    protected: Range,
    memory: Range,
    boundaries: &mut [usize],
    count: &mut usize,
) -> Result<(), MapError> {
    if protected.overlaps(memory) {
        push_boundary(boundaries, count, protected.start.max(memory.start))?;
        push_boundary(boundaries, count, protected.end.min(memory.end))?;
    }
    Ok(())
}

pub(super) fn push_boundary(
    boundaries: &mut [usize],
    count: &mut usize,
    boundary: usize,
) -> Result<(), MapError> {
    let slot = boundaries.get_mut(*count).ok_or(MapError::TooManyRanges)?;
    *slot = boundary;
    *count += 1;
    Ok(())
}

pub(super) fn dedup(values: &mut [usize], count: usize) -> usize {
    let mut unique = 0;
    for index in 0..count {
        if unique == 0 || values[index] != values[unique - 1] {
            values[unique] = values[index];
            unique += 1;
        }
    }
    unique
}

pub(super) fn emit(
    output: &mut [MemoryMapEntry],
    count: &mut usize,
    range: Range,
    ty: MemoryType,
) -> Result<(), MapError> {
    if *count > 0 {
        let previous = &mut output[*count - 1];
        if previous.ty == ty && previous.base + previous.length == range.start {
            previous.length = range.end - previous.base;
            return Ok(());
        }
    }
    let slot = output.get_mut(*count).ok_or(MapError::TooManyRanges)?;
    *slot = MemoryMapEntry {
        base: range.start,
        length: range.end - range.start,
        ty,
    };
    *count += 1;
    Ok(())
}
