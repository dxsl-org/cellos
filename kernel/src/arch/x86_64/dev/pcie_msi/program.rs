//! Device-side MSI / MSI-X programming for an allocated vector block.
//!
//! Every message targets the BSP with fixed delivery and edge trigger; the
//! vector doubles as the logical IRQ. Under interrupt remapping the message
//! names IRTEs at the VT-d unit covering the function instead.

use super::config::{Config, CFG_COMMAND, CMD_BUS_MASTER, CMD_INTX_DISABLE, CMD_MEM_SPACE};
use super::layout::MsixLayout;

const MSI_ENABLE: u16 = 1 << 0;
const MSI_64BIT: u16 = 1 << 7;
const MSI_PER_VECTOR_MASK: u16 = 1 << 8;
const MSI_MME_SHIFT: u16 = 4;
const MSIX_FUNCTION_MASK: u16 = 1 << 14;
const MSIX_ENABLE: u16 = 1 << 15;
const MSIX_ENTRY_BYTES: usize = 16;
const MSIX_VECTOR_MASKED: u32 = 1;

/// Largest MSI block the function accepts (Multiple Message Capable).
pub fn msi_capacity(control: u16) -> usize {
    1 << ((control >> 1) & 7).min(5)
}

/// MSI block size for `want`: rounded down to a power of two, capped by capacity.
pub fn msi_count(want: usize, control: u16) -> usize {
    let n = want.min(msi_capacity(control));
    if n == 0 {
        0
    } else {
        1 << (usize::BITS - 1 - n.leading_zeros())
    }
}

fn enable_function(cfg: &Config, extra: u16) -> Option<()> {
    let command = cfg.read16(CFG_COMMAND)?;
    cfg.write16(
        CFG_COMMAND,
        command | CMD_MEM_SPACE | CMD_BUS_MASTER | extra,
    )
}
/// Message for `count` vectors from `first` raised by function `bdf`.
/// `None` when no message can reach the BSP, so the caller fails the block
/// rather than hand out vectors that never fire.
fn msi_message(bdf: u32, first: u8, count: usize) -> Option<(u64, u32)> {
    use crate::arch::dev::iommu_x86::{ir, remap_msi, MsiRemap};
    if !ir::is_ir_active() {
        return hal::x86_64::msi::bsp_message(first);
    }
    let bsp_id = u8::try_from(hal::x86_64::apic::cpu_apic_id(0)?).ok()?;
    match remap_msi(bdf, first, count, bsp_id)? {
        MsiRemap::Compatible => hal::x86_64::msi::bsp_message(first),
        MsiRemap::Handle(handle) => Some((
            ir::remappable_msi_address(handle, count > 1),
            ir::remappable_msi_data(),
        )),
    }
}

/// Program `count` MSI-X entries starting at `first`, mask the rest of the
/// table, then enable MSI-X with INTx disabled. `table` is the kernel's alias
/// of the table (`paging::mmio_alias::kernel_mmio_alias` of BAR base + table offset), for
/// `table_size` entries.
pub fn program_msix(
    cfg: &Config,
    cap: u8,
    layout: &MsixLayout,
    table: usize,
    first: u8,
    count: usize,
) -> Option<()> {
    let control_at = cap as usize + 2;
    let control = cfg.read16(control_at)?;
    // Function-masked while entries change; MSI (if any) stays off.
    cfg.write16(control_at, control | MSIX_ENABLE | MSIX_FUNCTION_MASK)?;
    enable_function(cfg, CMD_INTX_DISABLE)?;
    let entries = layout.table_size as usize;
    for index in 0..entries {
        let entry = (table + index * MSIX_ENTRY_BYTES) as *mut u32;
        // A vector without a deliverable message fails the whole block; the
        // function stays masked.
        let message = if index < count {
            Some(msi_message(cfg.bdf(), first + index as u8, 1)?)
        } else {
            None
        };
        // SAFETY: the table lies inside the function's BAR (checked by the carve
        // rule) and the caller aliased it for the kernel.
        unsafe {
            core::ptr::write_volatile(entry.add(3), MSIX_VECTOR_MASKED);
            if let Some((address, data)) = message {
                core::ptr::write_volatile(entry, address as u32);
                core::ptr::write_volatile(entry.add(1), (address >> 32) as u32);
                core::ptr::write_volatile(entry.add(2), data);
                core::ptr::write_volatile(entry.add(3), 0);
            }
        }
    }
    cfg.write16(control_at, (control | MSIX_ENABLE) & !MSIX_FUNCTION_MASK)
}

/// Turn MSI-X off after [`program_msix`] failed partway: Enable clear and
/// Function Mask set, so the function is not left MSI-X-enabled with every
/// vector masked and can use MSI (or INTx) instead.
pub fn disable_msix(cfg: &Config, cap: u8) {
    let control_at = cap as usize + 2;
    if let Some(control) = cfg.read16(control_at) {
        let _ = cfg.write16(control_at, (control & !MSIX_ENABLE) | MSIX_FUNCTION_MASK);
    }
}

/// Program an MSI block of `count` (a power of two) vectors starting at the
/// `count`-aligned `first`, then enable MSI with INTx disabled.
pub fn program_msi(cfg: &Config, cap: u8, first: u8, count: usize) -> Option<()> {
    let base = cap as usize;
    let control = cfg.read16(base + 2)?;
    let (address, data) = msi_message(cfg.bdf(), first, count)?;
    cfg.write16(base + 2, control & !MSI_ENABLE)?;
    cfg.write32(base + 4, address as u32)?;
    let (data_at, mask_at) = if control & MSI_64BIT != 0 {
        cfg.write32(base + 8, (address >> 32) as u32)?;
        (base + 0x0C, base + 0x10)
    } else {
        (base + 0x08, base + 0x0C)
    };
    cfg.write16(data_at, data as u16)?;
    if control & MSI_PER_VECTOR_MASK != 0 {
        cfg.write32(mask_at, 0)?;
    }
    let mme = (count.trailing_zeros() as u16) << MSI_MME_SHIFT;
    enable_function(cfg, CMD_INTX_DISABLE)?;
    cfg.write16(
        base + 2,
        (control & !(7 << MSI_MME_SHIFT)) | mme | MSI_ENABLE,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn msi_rounds_down_to_power_of_two_within_capacity() {
        let mmc_8 = 3 << 1;
        assert_eq!(msi_capacity(mmc_8), 8);
        assert_eq!(msi_count(1, mmc_8), 1);
        assert_eq!(msi_count(3, mmc_8), 2);
        assert_eq!(msi_count(7, mmc_8), 4);
        assert_eq!(msi_count(32, mmc_8), 8);
        assert_eq!(msi_count(5, 0), 1, "single-message function");
        assert_eq!(msi_count(32, 7 << 1), 32, "reserved MMC values cap at 32");
    }
}
