//! VT-d Interrupt Remapping (Intel VT-d spec §5).

use core::sync::atomic::{fence, AtomicBool, AtomicU16, Ordering};

pub const DMAR_IRTA_REG: usize = 0xB8;

pub const VTD_GCMD_IRE: u32 = 1 << 25;
pub const VTD_GCMD_SIRTP: u32 = 1 << 24;
pub const VTD_GCMD_CFI: u32 = 1 << 23;

pub const VTD_GSTS_IRES: u32 = 1 << 25;
pub const VTD_GSTS_IRTPS: u32 = 1 << 24;
pub const VTD_GSTS_CFIS: u32 = 1 << 23;

pub const IRTA_EIM: u64 = 1 << 11; // Extended Interrupt Mode (32-bit APIC IDs)

static IR_ACTIVE: AtomicBool = AtomicBool::new(false);
static NEXT_IRTE_INDEX: AtomicU16 = AtomicU16::new(0);

pub fn is_ir_active() -> bool {
    IR_ACTIVE.load(Ordering::Acquire)
}

pub fn set_ir_active(active: bool) {
    IR_ACTIVE.store(active, Ordering::Release);
}

pub fn alloc_irte_index() -> Option<u16> {
    let idx = NEXT_IRTE_INDEX.fetch_add(1, Ordering::Relaxed);
    if idx < 4096 {
        Some(idx)
    } else {
        None
    }
}

/// 128-bit Interrupt Remapping Table Entry.
#[repr(C, align(16))]
#[derive(Clone, Copy, Debug, Default)]
pub struct Irte {
    pub lo: u64,
    pub hi: u64,
}

impl Irte {
    pub const fn new() -> Self {
        Self { lo: 0, hi: 0 }
    }

    pub fn encode(vector: u8, dest_apic_id: u32, sid: u16, level: bool) -> Self {
        // lo:
        // bit 0: present = 1
        // bit 1: fault_disable = 0
        // bit 2: dest_mode = 0 (physical)
        // bit 3: redir_hint = 0
        // bit 4: trigger_mode = 1 (level) or 0 (edge)
        // bits 7:5: delivery_mode = 000 (fixed)
        // bit 15: irte_mode = 0 (remapped)
        // bits 23:16: vector
        // bits 63:32: dest_id (32-bit APIC ID in EIM/x2APIC mode)
        let trigger = if level { 1u64 << 4 } else { 0 };
        let lo = 1u64 | trigger | ((vector as u64) << 16) | ((dest_apic_id as u64) << 32);

        // hi:
        // bits 15:0: source_id = sid
        // bits 17:16: sid_q = 00 (exact)
        // bits 19:18: sid_vtype = 01 (validate SID)
        let hi = (sid as u64) | (1u64 << 18);

        Self { lo, hi }
    }
}

pub struct IrTable {
    pub virt: usize,
    pub phys: u64,
    pub entries: u32,
    pub size_power: u8, // size = 2^(size_power + 1)
}

#[inline]
fn virt_to_phys(virt: usize) -> u64 {
    #[cfg(target_arch = "x86_64")]
    {
        (virt - crate::memory::frame::phys_to_virt(0)) as u64
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        virt as u64
    }
}
impl IrTable {
    pub fn new(size_power: u8) -> Self {
        let entries = 1u32 << (size_power + 1);
        let bytes = (entries as usize) * 16;
        let align = bytes.next_power_of_two().max(4096);
        let layout = alloc::alloc::Layout::from_size_align(bytes, align).expect("irta layout");
        let virt = unsafe { alloc::alloc::alloc_zeroed(layout) as usize };
        if virt == 0 {
            panic!("[vtd] out of memory allocating IR table");
        }
        let phys = virt_to_phys(virt);
        Self {
            virt,
            phys,
            entries,
            size_power,
        }
    }

    pub fn program_entry(&self, index: u16, irte: Irte) {
        if (index as u32) >= self.entries {
            return;
        }
        let ptr = (self.virt + (index as usize) * 16) as *mut u64;
        unsafe {
            core::ptr::write_volatile(ptr.add(1), irte.hi);
            fence(Ordering::Release);
            core::ptr::write_volatile(ptr, irte.lo);
        }
    }
}

pub fn remappable_msi_address(index: u16) -> u64 {
    // 0xFEE0_0000 | (1 << 4) | (index_l << 5) | (index_h << 2)
    let index_l = (index & 0x7FFF) as u64;
    let index_h = ((index >> 15) & 1) as u64;
    0xFEE0_0000u64 | (1 << 4) | (index_l << 5) | (index_h << 2)
}

pub fn remappable_msi_data() -> u32 {
    0
}

pub fn remappable_ioapic_entry(
    index: u16,
    masked: bool,
    trigger_level: bool,
    active_low: bool,
) -> u64 {
    let index_l = (index & 0x7FFF) as u64;
    let index_h = ((index >> 15) & 1) as u64;
    let mask_bit = if masked { 1u64 << 16 } else { 0 };
    let trig_bit = if trigger_level { 1u64 << 15 } else { 0 };
    let pol_bit = if active_low { 1u64 << 13 } else { 0 };
    let int_mode = 1u64 << 48; // format: remapped
    int_mode | (index_l << 49) | (index_h << 11) | mask_bit | trig_bit | pol_bit
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn irte_encoding_and_fields() {
        let entry = Irte::encode(0x42, 0x105, 0x0100, false);
        assert_eq!(entry.lo & 1, 1); // present
        assert_eq!((entry.lo >> 16) & 0xFF, 0x42); // vector
        assert_eq!((entry.lo >> 32) & 0xFFFF_FFFF, 0x105); // dest_id
        assert_eq!(entry.hi & 0xFFFF, 0x0100); // sid
        assert_eq!((entry.hi >> 18) & 0x3, 1); // sid_vtype
    }

    #[test]
    fn remappable_msi_address_encoding() {
        let addr = remappable_msi_address(0x123);
        assert_eq!(addr & 0xFFF0_0000, 0xFEE0_0000);
        assert_eq!((addr >> 4) & 1, 1); // int_mode
        assert_eq!((addr >> 5) & 0x7FFF, 0x123);
        assert_eq!((addr >> 2) & 1, 0);

        let addr_hi = remappable_msi_address(0x8005);
        assert_eq!((addr_hi >> 5) & 0x7FFF, 5);
        assert_eq!((addr_hi >> 2) & 1, 1);
    }
}
