//! SV39 Paging Implementation for RV64
use hal_paging::{PageFlags, PageTableTrait};
use types::*;

pub const PAGE_SIZE: usize = 4096;

/// Helper to map Generic Flags to RISC-V Flags
fn to_riscv_flags(flags: PageFlags) -> usize {
    let mut bits = flags.bits();

    // Ensure VALID is set if any other bit is set (safeguard)
    if bits != 0 {
        bits |= 1 << 0;
    }

    bits
}

/// A Page Table Entry (64-bit)
#[derive(Debug, Clone, Copy)]
#[repr(transparent)]
struct PageTableEntry(usize);

impl PageTableEntry {
    #[allow(dead_code)]
    fn new(addr: PhysAddr, flags: usize) -> Self {
        let ppn = (addr >> 12) & 0x003F_FFFF_FFFF_FFFF;
        Self((ppn << 10) | flags)
    }

    fn is_valid(&self) -> bool {
        (self.0 & 1) != 0
    }

    fn is_leaf(&self) -> bool {
        // R=1, W=1, X=1 in any combination check
        let rwx = (self.0 >> 1) & 0x7;
        rwx != 0
    }

    fn to_phys(&self) -> PhysAddr {
        ((self.0 >> 10) & 0x003F_FFFF_FFFF_FFFF) << 12
    }

    fn set(&mut self, addr: PhysAddr, flags: usize) {
        let ppn = (addr >> 12) & 0x003F_FFFF_FFFF_FFFF;
        self.0 = (ppn << 10) | flags;
    }
}

/// A 4KB Page Table (512 entries)
#[repr(C, align(4096))]
pub struct PageTable {
    entries: [PageTableEntry; 512],
}

impl PageTable {
    pub fn empty() -> Self {
        Self {
            entries: [PageTableEntry(0); 512],
        }
    }
}

impl PageTableTrait for PageTable {
    fn init(&mut self) -> ViResult<PhysAddr> {
        // In this design, the PageTable struct IS the table.
        // But usually we manage the pointer to the root table.
        // For simpler adaptation, we assume `self` IS the root table in generic memory.
        // This might be tricky if `self` is on stack.
        // However, the Kernel wraps this.
        // For now, we return 0 or the address if we knew it.
        // But `PageTableTrait` design implies we might alloc root here?
        // Let's assume the caller manages the memory of the generic struct wrapper,
        // but the actual hardware table is what matters.
        Err(ViError::NotSupported) // TODO: Refine trait semantics
    }

    fn map(
        &mut self,
        virt: VAddr,
        phys: PhysAddr,
        flags: PageFlags,
        alloc_fn: &mut dyn FnMut() -> Option<PhysAddr>,
    ) -> ViResult<()> {
        // VPN[2] -> VPN[1] -> VPN[0]
        let mut table = self;

        for level in (1..3).rev() {
            // Levels 2, 1
            let shift = 12 + (level * 9);
            let vpn = (virt >> shift) & 0x1FF;
            let mut entry = table.entries[vpn];

            if !entry.is_valid() {
                let frame_addr = alloc_fn().ok_or(ViError::OutOfMemory)?;
                // Valid, generic "Directory" flags
                // V=1, R=0, W=0, X=0
                entry.set(frame_addr, 1);
                table.entries[vpn] = entry;

                // Zeroing is handled by alloc_fn ideally, or we do it here safely?
                // Since we are in HAL, we might be running Identity Mapped code.
                // We can cast PhysAddr to *mut u8 if mapped.
                unsafe {
                    let ptr = frame_addr as *mut u8;
                    core::ptr::write_bytes(ptr, 0, PAGE_SIZE);
                }
            }

            let next_table_phys = entry.to_phys();
            table = unsafe { &mut *(next_table_phys as *mut PageTable) };
        }

        // Leaf (Level 0)
        let vpn = (virt >> 12) & 0x1FF;
        table.entries[vpn].set(phys, to_riscv_flags(flags));
        Ok(())
    }

    fn unmap(&mut self, virt: VAddr) -> ViResult<()> {
        let mut table = self;
        for level in (1..3).rev() {
            let shift = 12 + (level * 9);
            let vpn = (virt >> shift) & 0x1FF;
            let entry = table.entries[vpn];
            if !entry.is_valid() || entry.is_leaf() {
                return Err(ViError::NotFound);
            }
            let next_table_phys = entry.to_phys();
            table = unsafe { &mut *(next_table_phys as *mut PageTable) };
        }
        let vpn = (virt >> 12) & 0x1FF;
        table.entries[vpn] = PageTableEntry(0); // Invalidate
                                                // Flush TLB?
        unsafe {
            core::arch::asm!("sfence.vma");
        }
        Ok(())
    }

    fn translate(&self, virt: VAddr) -> Option<PhysAddr> {
        let mut table = self;
        for level in (0..3).rev() {
            let shift = 12 + (level * 9);
            let vpn = (virt >> shift) & 0x1FF;
            let entry = table.entries[vpn];

            if !entry.is_valid() {
                return None;
            }

            if entry.is_leaf() {
                let offset_mask = (1 << shift) - 1;
                let offset = virt & offset_mask;
                return Some(entry.to_phys() | offset);
            }

            let next_table_phys = entry.to_phys();
            table = unsafe { &*(next_table_phys as *const PageTable) };
        }
        None
    }

    unsafe fn activate(&self) {
        let root_addr = self as *const _ as usize;
        let satp_val = (8usize << 60) | (root_addr >> 12);
        // SAFETY: fence ensures all PTE stores reach the memory system before the
        // SATP write, preventing the hardware page-walker from seeing stale entries.
        // Required by RISC-V privileged spec §4.3 (sfence.vma ordering).
        core::arch::asm!(
            "fence rw, rw",
            "csrw satp, {satp}",
            "sfence.vma zero, zero",
            satp = in(reg) satp_val,
            options(nostack),
        );
    }
}
