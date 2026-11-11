//! Interrupt Remapping Table storage and entry allocation.
//!
//! Entries are handed out from a bump cursor. MSI runs are never freed but an
//! identical run is reused. Each I/O APIC pin owns one entry for life and is
//! rewritten in place when its route changes; the caller then invalidates the
//! interrupt entry cache for that index.

use super::super::cache::sync_for;
use super::Irte;
use alloc::collections::BTreeMap;
use core::sync::atomic::{fence, Ordering};

/// An entry handed out by the table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    /// Interrupt index (handle) of the first entry.
    pub index: u16,
    /// Whether any entry was written, so the interrupt entry cache must be
    /// invalidated before hardware relies on it.
    pub written: bool,
}

pub struct IrTable {
    pub virt: usize,
    pub phys: u64,
    pub entries: u32,
    pub size_power: u8, // size = 2^(size_power + 1)
    /// `ECAP` of the owning unit: a non-snooping unit needs each store flushed.
    ecap: u64,
    /// Entries `[0, used)` have been handed out; the rest are not present.
    used: u32,
    /// `(I/O APIC id, pin)` → its entry.
    ioapic: BTreeMap<(u8, u8), u16>,
}

impl IrTable {
    pub fn new(size_power: u8, ecap: u64) -> Self {
        let entries = 1u32 << (size_power + 1);
        let bytes = (entries as usize) * 16;
        let align = bytes.next_power_of_two().max(4096);
        let layout = alloc::alloc::Layout::from_size_align(bytes, align).expect("irta layout");
        // SAFETY: the layout is non-zero sized.
        let virt = unsafe { alloc::alloc::alloc_zeroed(layout) as usize };
        if virt == 0 {
            panic!("[vtd] out of memory allocating IR table");
        }
        // The zeroed (not-present) entries must reach memory too.
        sync_for(ecap, virt, bytes);
        let phys = super::super::super::iommu_pt::virt_to_phys(virt);
        Self {
            virt,
            phys,
            entries,
            size_power,
            ecap,
            used: 0,
            ioapic: BTreeMap::new(),
        }
    }

    fn read_entry(&self, index: u32) -> Irte {
        let ptr = (self.virt + (index as usize) * 16) as *const u64;
        // SAFETY: index < used <= entries, inside the allocated table.
        unsafe {
            Irte {
                lo: core::ptr::read_volatile(ptr),
                hi: core::ptr::read_volatile(ptr.add(1)),
            }
        }
    }

    /// Store `irte` at `index`. The high half (requester id) goes first and
    /// the present bit last, in one 64-bit store; a present entry is only
    /// rewritten with an unchanged high half, so hardware never sees a torn
    /// entry.
    fn write_entry(&self, index: u32, irte: Irte) {
        let ptr = (self.virt + (index as usize) * 16) as *mut u64;
        // SAFETY: index < entries, inside the allocated table.
        unsafe {
            core::ptr::write_volatile(ptr.add(1), irte.hi);
            fence(Ordering::Release);
            core::ptr::write_volatile(ptr, irte.lo);
        }
        sync_for(self.ecap, ptr as usize, 16);
    }

    /// Whether `[start, start + count)` holds an I/O APIC pin's entry.
    fn overlaps_ioapic(&self, start: u32, count: u32) -> bool {
        self.ioapic
            .values()
            .any(|&index| (start..start + count).contains(&(index as u32)))
    }

    /// `count` consecutive entries remapping vectors
    /// `first_vector..first_vector+count` for requester `sid` to `dest_apic_id`
    /// (edge-triggered). An identical run already present is reused, so a
    /// restarted driver that gets its old vectors back costs no entries.
    /// `None` when the table has no room or a vector would overflow.
    pub fn install(
        &mut self,
        first_vector: u8,
        count: usize,
        dest_apic_id: u8,
        sid: u16,
    ) -> Option<Slot> {
        let count = u32::try_from(count).ok().filter(|&c| c != 0)?;
        let entry = |i: u32| {
            let vector = first_vector.checked_add(u8::try_from(i).ok()?)?;
            Some(Irte::encode(vector, dest_apic_id, sid, false))
        };
        entry(count - 1)?;
        let reused = self.used.checked_sub(count).and_then(|last| {
            (0..=last).find(|&start| {
                !self.overlaps_ioapic(start, count)
                    && (0..count).all(|i| Some(self.read_entry(start + i)) == entry(i))
            })
        });
        if let Some(start) = reused {
            let index = u16::try_from(start).ok()?;
            return Some(Slot {
                index,
                written: false,
            });
        }
        let start = self.used;
        if start.checked_add(count)? > self.entries {
            return None;
        }
        let index = u16::try_from(start).ok()?;
        for i in 0..count {
            self.write_entry(start + i, entry(i)?);
        }
        self.used = start + count;
        Some(Slot {
            index,
            written: true,
        })
    }

    /// The entry owned by pin `pin` of the I/O APIC with MADT id `ioapic_id`,
    /// now holding `irte`. `None` when a new entry does not fit.
    pub fn install_ioapic(&mut self, ioapic_id: u8, pin: u8, irte: Irte) -> Option<Slot> {
        let index = match self.ioapic.get(&(ioapic_id, pin)) {
            Some(&index) => index,
            None => {
                if self.used >= self.entries {
                    return None;
                }
                let index = u16::try_from(self.used).ok()?;
                self.used += 1;
                self.ioapic.insert((ioapic_id, pin), index);
                index
            }
        };
        let current = self.read_entry(index as u32);
        let written = current != irte;
        if written {
            // The pin's requester id never changes, so a present entry is
            // rewritten with the same high half (see `write_entry`).
            debug_assert!(current.lo & 1 == 0 || current.hi == irte.hi);
            self.write_entry(index as u32, irte);
        }
        Some(Slot { index, written })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot(index: u16, written: bool) -> Option<Slot> {
        Some(Slot { index, written })
    }

    fn index(slot: Option<Slot>) -> Option<u16> {
        slot.map(|s| s.index)
    }

    #[test]
    fn install_reuses_identical_runs_and_bounds_the_table() {
        let mut table = IrTable::new(1, 1); // 4 entries, coherent
        assert_eq!(table.install(0x40, 2, 0, 0x0018), slot(0, true));
        assert_eq!(table.read_entry(1), Irte::encode(0x41, 0, 0x0018, false));
        let suffix = table.install(0x41, 1, 0, 0x0018);
        assert_eq!(suffix, slot(1, false), "suffix reused");
        let other_sid = table.install(0x40, 1, 0, 0x0020);
        assert_eq!(index(other_sid), Some(2));
        assert_eq!(index(table.install(0x50, 2, 0, 0x0020)), None, "no room");
        assert_eq!(index(table.install(0x50, 1, 0, 0x0020)), Some(3));
        assert_eq!(index(table.install(0x40, 2, 0, 0x0018)), Some(0));
        let overflow = table.install(0xFF, 2, 0, 0x0018);
        assert_eq!(index(overflow), None, "vector overflow");
        assert_eq!(index(table.install(0x40, 0, 0, 0x0018)), None);
    }

    #[test]
    fn ioapic_pins_own_one_entry_rewritten_in_place() {
        let mut table = IrTable::new(1, 0); // 4 entries, non-snooping
        let uart = Irte::encode(0x24, 0, 0xF0F8, false);
        assert_eq!(table.install_ioapic(0, 4, uart), slot(0, true));
        assert_eq!(table.install_ioapic(0, 4, uart), slot(0, false));
        let moved = Irte::encode(0x25, 1, 0xF0F8, true);
        assert_eq!(table.install_ioapic(0, 4, moved), slot(0, true));
        assert_eq!(table.read_entry(0), moved);
        assert_eq!(index(table.install_ioapic(0, 9, uart)), Some(1));
        // An MSI run never reuses a pin's entry, even an identical one.
        assert_eq!(table.read_entry(1), Irte::encode(0x24, 0, 0xF0F8, false));
        assert_eq!(index(table.install(0x24, 1, 0, 0xF0F8)), Some(2));
        assert_eq!(index(table.install_ioapic(1, 0, uart)), Some(3));
        assert_eq!(index(table.install_ioapic(1, 1, uart)), None, "full");
    }
}
