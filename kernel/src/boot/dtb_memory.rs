use super::{MemoryMapEntry, MemoryType};

#[path = "dtb_memory_ranges.rs"]
mod ranges;
use ranges::*;

const PAGE_SIZE: usize = 4096;
const MAX_RANGES: usize = super::MAX_MEMORY_MAP_ENTRIES;
const MAX_BOUNDARIES: usize = MAX_RANGES * 2 + 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapError {
    NoMemory,
    InvalidRange,
    KernelOutsideMemory,
    DynamicReservation,
    TooManyRanges,
    NoUsableMemory,
}

pub fn build(
    tree: &fdt::Fdt<'_>,
    kernel_base: usize,
    kernel_end: usize,
    output: &mut [MemoryMapEntry],
) -> Result<usize, MapError> {
    let kernel_end = align_up(kernel_end, PAGE_SIZE)?;
    if kernel_base >= kernel_end {
        return Err(MapError::InvalidRange);
    }

    let mut ram = [Range::default(); MAX_RANGES];
    let mut ram_count = 0;
    for node in tree.all_nodes() {
        if node.name.split('@').next() != Some("memory") || !is_enabled(&node) {
            continue;
        }
        let regions = node.reg().ok_or(MapError::InvalidRange)?;
        for region in regions {
            push_range(
                &mut ram,
                &mut ram_count,
                Range::ram(
                    region.starting_address as usize,
                    region.size.ok_or(MapError::InvalidRange)?,
                )?,
            )?;
        }
    }
    if ram_count == 0 {
        return Err(MapError::NoMemory);
    }
    sort_ranges(&mut ram[..ram_count]);
    ram_count = merge_ranges(&mut ram, ram_count);

    let kernel = Range {
        start: kernel_base,
        end: kernel_end,
    };
    let kernel_ram = ram[..ram_count]
        .iter()
        .copied()
        .find(|range| range.start <= kernel.start && kernel.end <= range.end)
        .ok_or(MapError::KernelOutsideMemory)?;
    let firmware = Range {
        start: kernel_ram.start,
        end: kernel_base,
    };

    let mut reserved = [Range::default(); MAX_RANGES];
    let mut reserved_count = 0;
    for reservation in tree.memory_reservations() {
        push_range(
            &mut reserved,
            &mut reserved_count,
            Range::protected(reservation.address() as usize, reservation.size())?,
        )?;
    }
    if let Some(parent) = tree.find_node("/reserved-memory") {
        for child in parent.children().filter(is_enabled) {
            let Some(regions) = child.reg() else {
                if child.property("size").is_some() {
                    return Err(MapError::DynamicReservation);
                }
                continue;
            };
            let mut found = false;
            for region in regions {
                found = true;
                push_range(
                    &mut reserved,
                    &mut reserved_count,
                    Range::protected(
                        region.starting_address as usize,
                        region.size.ok_or(MapError::InvalidRange)?,
                    )?,
                )?;
            }
            if !found && child.property("size").is_some() {
                return Err(MapError::DynamicReservation);
            }
        }
    }

    let mut output_count = 0;
    let mut usable_count = 0;
    for memory in ram[..ram_count].iter().copied() {
        let mut boundaries = [0usize; MAX_BOUNDARIES];
        let mut boundary_count = 0;
        push_boundary(&mut boundaries, &mut boundary_count, memory.start)?;
        push_boundary(&mut boundaries, &mut boundary_count, memory.end)?;
        for protected in reserved[..reserved_count].iter().copied() {
            add_boundaries(protected, memory, &mut boundaries, &mut boundary_count)?;
        }
        for protected in [firmware, kernel] {
            add_boundaries(protected, memory, &mut boundaries, &mut boundary_count)?;
        }
        boundaries[..boundary_count].sort_unstable();
        boundary_count = dedup(&mut boundaries, boundary_count);

        for pair in boundaries[..boundary_count].windows(2) {
            let segment = Range {
                start: pair[0],
                end: pair[1],
            };
            if segment.start == segment.end {
                continue;
            }
            let ty = if segment.overlaps(kernel) {
                MemoryType::Kernel
            } else if reserved[..reserved_count]
                .iter()
                .copied()
                .any(|range| range.overlaps(segment))
            {
                MemoryType::Reserved
            } else if segment.overlaps(firmware) {
                MemoryType::Bootloader
            } else {
                usable_count += 1;
                MemoryType::Usable
            };
            emit(output, &mut output_count, segment, ty)?;
        }
    }
    if usable_count == 0 {
        return Err(MapError::NoUsableMemory);
    }
    Ok(output_count)
}
