//! MSI-X table/PBA placement and the page carve-out rule (CP1 §4.3).
//!
//! `RequestMmio` maps a BAR without the pages that hold the MSI-X table or PBA,
//! so a cell cannot retarget its device's interrupts. The carve is page-granular,
//! so it is only safe when those pages hold nothing but MSI-X structures. The
//! PCIe base spec requires exactly that (no other CSR in a naturally aligned 4 KiB
//! range holding the table or PBA); what software can check is that every page
//! touched starts with an MSI-X structure. A layout that fails is not carved and
//! is not offered MSI-X, so its driver keeps its full BAR and polls or uses MSI.

const PAGE: u64 = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MsixLayout {
    /// Entries (N, not N-1).
    pub table_size: u16,
    pub table_bar: u8,
    pub table_offset: u32,
    pub pba_bar: u8,
    pub pba_offset: u32,
}

impl MsixLayout {
    /// Decode Message Control, Table Offset/BIR and PBA Offset/BIR registers.
    pub fn from_registers(control: u16, table: u32, pba: u32) -> Self {
        Self {
            table_size: (control & 0x7FF) + 1,
            table_bar: (table & 7) as u8,
            table_offset: table & !7,
            pba_bar: (pba & 7) as u8,
            pba_offset: pba & !7,
        }
    }

    fn structures(&self) -> [(u8, u64, u64); 2] {
        let n = self.table_size as u64;
        [
            (self.table_bar, self.table_offset as u64, n * 16),
            (self.pba_bar, self.pba_offset as u64, n.div_ceil(64) * 8),
        ]
    }

    /// Whether both structures sit inside their BARs (`bar_len(i)` = size of BAR
    /// `i`, 0 when absent or I/O) and every page they touch starts with one.
    pub fn carve_safe(&self, bar_len: impl Fn(u8) -> u64) -> bool {
        let structs = self.structures();
        structs.iter().all(|&(bar, off, len)| {
            let inside = bar < 6 && off.checked_add(len).is_some_and(|end| end <= bar_len(bar));
            let page = off & !(PAGE - 1);
            let page_starts_with_msix = off == page
                || structs
                    .iter()
                    .any(|&(b, o, _)| b == bar && o == page && o < off);
            inside && page_starts_with_msix
        })
    }

    /// BAR-relative page ranges `[start, end)` carved from BAR `bar`.
    pub fn carved_ranges(&self, bar: u8) -> impl Iterator<Item = (u64, u64)> {
        self.structures()
            .into_iter()
            .filter(move |&(b, _, len)| b == bar && len != 0)
            .map(|(_, off, len)| (off & !(PAGE - 1), (off + len).div_ceil(PAGE) * PAGE))
    }

    /// Whether BAR `bar` holds the table or the PBA.
    pub fn holds(&self, bar: u8) -> bool {
        self.table_bar == bar || self.pba_bar == bar
    }
}

/// `[start, end)` minus every hole, as ascending disjoint ranges. Holes may
/// overlap each other or extend past the range.
pub fn subtract(start: u64, end: u64, holes: &[(u64, u64)]) -> alloc::vec::Vec<(u64, u64)> {
    let mut sorted: alloc::vec::Vec<(u64, u64)> = holes.to_vec();
    sorted.sort_unstable();
    let mut out = alloc::vec::Vec::new();
    let mut cursor = start;
    for (hole_start, hole_end) in sorted {
        if hole_start > cursor {
            out.push((cursor, hole_start.min(end)));
        }
        cursor = cursor.max(hole_end);
        if cursor >= end {
            break;
        }
    }
    if cursor < end {
        out.push((cursor, end));
    }
    out.retain(|(s, e)| s < e);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bars(lens: [u64; 6]) -> impl Fn(u8) -> u64 {
        move |i| lens.get(i as usize).copied().unwrap_or(0)
    }

    #[test]
    fn decodes_registers() {
        let l = MsixLayout::from_registers(0x8002, 0x0000_0001, 0x0000_0801);
        assert_eq!(l.table_size, 3);
        assert_eq!((l.table_bar, l.table_offset), (1, 0));
        assert_eq!((l.pba_bar, l.pba_offset), (1, 0x800));
    }

    #[test]
    fn dedicated_msix_bar_with_shared_page_is_safe() {
        // virtio-pci on QEMU: BAR1 = table @0, PBA @0x800, one page.
        let l = MsixLayout::from_registers(2, 1, 0x801);
        assert!(l.carve_safe(bars([0, 0x1000, 0, 0, 0, 0])));
        assert_eq!(
            l.carved_ranges(1).collect::<alloc::vec::Vec<_>>(),
            [(0, 0x1000), (0, 0x1000)]
        );
        assert_eq!(l.carved_ranges(0).count(), 0);
    }

    #[test]
    fn page_aligned_structures_after_registers_are_safe() {
        // NVMe-style: registers, then table @0x2000 and PBA @0x3000 in BAR0.
        let l = MsixLayout::from_registers(63, 0x2000, 0x3000);
        assert!(l.carve_safe(bars([0x4000, 0, 0, 0, 0, 0])));
        assert_eq!(
            l.carved_ranges(0).collect::<alloc::vec::Vec<_>>(),
            [(0x2000, 0x3000), (0x3000, 0x4000)]
        );
    }

    #[test]
    fn table_sharing_a_page_with_registers_is_refused() {
        // Table @0x800 with registers at 0..0x800 of the same page.
        let l = MsixLayout::from_registers(7, 0x800, 0x2000);
        assert!(!l.carve_safe(bars([0x4000, 0, 0, 0, 0, 0])));
    }

    #[test]
    fn structures_outside_their_bar_are_refused() {
        let l = MsixLayout::from_registers(7, 0x2000, 0x3000);
        assert!(
            !l.carve_safe(bars([0x3000, 0, 0, 0, 0, 0])),
            "PBA past BAR end"
        );
        let l = MsixLayout::from_registers(7, 6, 0x1006);
        assert!(!l.carve_safe(bars([0; 6])), "BIR 6 is reserved");
    }

    #[test]
    fn subtract_removes_overlapping_and_out_of_range_holes() {
        assert_eq!(subtract(0, 0x4000, &[]), [(0, 0x4000)]);
        assert_eq!(
            subtract(0, 0x4000, &[(0x3000, 0x4000), (0x2000, 0x3000)]),
            [(0, 0x2000)]
        );
        assert!(subtract(0, 0x1000, &[(0, 0x1000), (0, 0x1000)]).is_empty());
        assert_eq!(
            subtract(
                0x1000,
                0x5000,
                &[(0, 0x2000), (0x3000, 0x3800), (0x9000, 0xA000)]
            ),
            [(0x2000, 0x3000), (0x3800, 0x5000)]
        );
    }
}
