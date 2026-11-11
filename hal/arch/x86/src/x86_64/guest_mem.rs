// SPDX-License-Identifier: MIT
//! Guest-RAM reads for exit emulation, shared by the SVM and VMX backends.
//!
//! A bounds-checked view of the contiguous guest carve plus the 4-level guest
//! page walk needed to fetch the instruction behind an MMIO fault. The pinned
//! x86 guest contract is a PVH Linux guest: MMIO is reached either with paging
//! off (linear == physical, flat segments) or in 4-level long mode; LA57 and
//! paging outside long mode (EFER.LMA=0) are refused rather than guessed.

const CR0_PG: u64 = 1 << 31;
const CR4_LA57: u64 = 1 << 12;
const EFER_LMA: u64 = 1 << 10;
const ADDRESS_MASK_4K: u64 = 0x000f_ffff_ffff_f000;
const ADDRESS_MASK_2M: u64 = 0x000f_ffff_ffe0_0000;
const ADDRESS_MASK_1G: u64 = 0x000f_ffff_c000_0000;
const PTE_PRESENT: u64 = 1;
const PTE_LARGE: u64 = 1 << 7;

/// Longest x86 instruction.
pub const MAX_INSTRUCTION_LEN: usize = 15;

/// Read-only view of the guest's contiguous RAM (guest-physical 0 at `base`).
pub struct GuestRam {
    base: *const u8,
    len: usize,
}

impl GuestRam {
    /// # Safety
    /// `base..base+len` must stay mapped and readable for the view's lifetime
    /// (the kernel VM entry owns the carve and outlives every vCPU).
    pub const unsafe fn new(base: *const u8, len: usize) -> Self {
        Self { base, len }
    }

    /// Byte at guest-physical `gpa`, or `None` outside the carve.
    pub fn read_u8(&self, gpa: u64) -> Option<u8> {
        let offset: usize = gpa.try_into().ok()?;
        if offset >= self.len {
            return None;
        }
        // SAFETY: constructor contract keeps the carve readable; the bounds
        // check keeps the read inside it.
        Some(unsafe { core::ptr::read(self.base.add(offset)) })
    }

    fn read_u64(&self, gpa: u64) -> Option<u64> {
        let mut bytes = [0u8; 8];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = self.read_u8(gpa.checked_add(index as u64)?)?;
        }
        Some(u64::from_le_bytes(bytes))
    }

    /// Guest-physical address of linear `address` under the guest's
    /// CR0/CR3/CR4/EFER.
    pub fn translate(&self, cr0: u64, cr3: u64, cr4: u64, efer: u64, address: u64) -> Option<u64> {
        if cr0 & CR0_PG == 0 {
            return (address < self.len as u64).then_some(address);
        }
        if cr4 & CR4_LA57 != 0 || efer & EFER_LMA == 0 {
            return None;
        }
        let mut table = cr3 & ADDRESS_MASK_4K;
        for (level, shift) in [(4u8, 39u8), (3, 30), (2, 21), (1, 12)] {
            let index = (address >> shift) & 0x1ff;
            let entry = self.read_u64(table.checked_add(index * 8)?)?;
            if entry & PTE_PRESENT == 0 {
                return None;
            }
            if entry & PTE_LARGE != 0 {
                return match level {
                    3 => Some((entry & ADDRESS_MASK_1G) | (address & ((1 << 30) - 1))),
                    2 => Some((entry & ADDRESS_MASK_2M) | (address & ((1 << 21) - 1))),
                    _ => None,
                };
            }
            table = entry & ADDRESS_MASK_4K;
        }
        Some(table | (address & 0xfff))
    }

    /// Up to [`MAX_INSTRUCTION_LEN`] bytes at guest linear `rip`; stops at the
    /// first byte that is unmapped or outside the carve. `None` if none is readable.
    pub fn fetch_instruction(
        &self,
        cr0: u64,
        cr3: u64,
        cr4: u64,
        efer: u64,
        rip: u64,
    ) -> Option<([u8; MAX_INSTRUCTION_LEN], usize)> {
        let mut bytes = [0u8; MAX_INSTRUCTION_LEN];
        let mut available = 0;
        for (index, byte) in bytes.iter_mut().enumerate() {
            let Some(value) = rip
                .checked_add(index as u64)
                .and_then(|address| self.translate(cr0, cr3, cr4, efer, address))
                .and_then(|gpa| self.read_u8(gpa))
            else {
                break;
            };
            *byte = value;
            available += 1;
        }
        (available != 0).then_some((bytes, available))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PML4: u64 = 0x1000;
    const PDPT: u64 = 0x2000;
    const PD: u64 = 0x3000;
    const PT: u64 = 0x4000;

    fn put(ram: &mut [u8], gpa: u64, value: u64) {
        ram[gpa as usize..gpa as usize + 8].copy_from_slice(&value.to_le_bytes());
    }

    /// Linear 0xffff_8000_0040_1234 -> 4 KiB page at 0x9000; linear 0x20_0000 +
    /// x -> 2 MiB page at 0x0 (identity); linear 0x4000_0000 + x -> 1 GiB page at 0.
    fn tables() -> [u8; 0x10000] {
        let mut ram = [0u8; 0x10000];
        let high = 0xffff_8000_0040_1234u64;
        put(&mut ram, PML4 + ((high >> 39) & 0x1ff) * 8, PDPT | 3);
        put(&mut ram, PDPT + ((high >> 30) & 0x1ff) * 8, PD | 3);
        put(&mut ram, PD + ((high >> 21) & 0x1ff) * 8, PT | 3);
        put(&mut ram, PT + ((high >> 12) & 0x1ff) * 8, 0x9000 | 3);
        put(&mut ram, PML4, PDPT | 3);
        put(&mut ram, PDPT, PD | 3);
        put(&mut ram, PD + 8, 3 | PTE_LARGE);
        put(&mut ram, PDPT + 8, 3 | PTE_LARGE);
        ram
    }

    #[test]
    fn walks_4k_2m_and_1g_mappings_and_refuses_holes() {
        let ram = tables();
        let view = unsafe { GuestRam::new(ram.as_ptr(), ram.len()) };
        let t = |address| view.translate(CR0_PG, PML4, 0, EFER_LMA, address);
        assert_eq!(t(0xffff_8000_0040_1234), Some(0x9234));
        assert_eq!(t(0x20_0000 + 0x1_2345), Some(0x1_2345));
        assert_eq!(t(0x4000_0000 + 0x0abc_def0), Some(0x0abc_def0));
        assert_eq!(t(0x60_0000), None, "unmapped PD entry");
        assert_eq!(view.translate(CR0_PG, PML4, CR4_LA57, EFER_LMA, 0x20_0000), None);
        assert_eq!(view.translate(CR0_PG, PML4, 0, 0, 0x20_0000), None, "paging outside long mode");
    }

    #[test]
    fn paging_off_is_identity_inside_the_carve_only() {
        let ram = [0u8; 0x100];
        let view = unsafe { GuestRam::new(ram.as_ptr(), ram.len()) };
        assert_eq!(view.translate(0, 0, 0, 0, 0xff), Some(0xff));
        assert_eq!(view.translate(0, 0, 0, 0, 0x100), None);
    }

    #[test]
    fn fetch_stops_at_the_end_of_the_carve() {
        let mut ram = [0u8; 0x100];
        ram[0xfc..].copy_from_slice(&[0x89, 0x07, 0x90, 0x90]);
        let view = unsafe { GuestRam::new(ram.as_ptr(), ram.len()) };
        let (bytes, len) = view.fetch_instruction(0, 0, 0, 0, 0xfc).unwrap();
        assert_eq!((len, &bytes[..2]), (4, &[0x89, 0x07][..]));
        assert!(view.fetch_instruction(0, 0, 0, 0, 0x100).is_none());
    }
}
