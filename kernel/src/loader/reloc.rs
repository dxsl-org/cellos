//! ELF relocation engine for ViCell cells.
//!
//! Supports the relocation types emitted by the RISC-V LLVM/GCC toolchain
//! when building position-independent cells (`-pie` or `-shared`).

use types::{VAddr, ViError, ViResult};

/// Maximum number of relocations to process per cell.
/// Bounds parsing time; a legitimate cell has at most tens of thousands.
const MAX_RELA_ENTRIES: usize = 65_536;

/// RISC-V ELF relocation types (r_type field in Elf64_Rela.r_info).
#[allow(dead_code)] // reason: table documents all types; only a subset is used
mod riscv_reloc_type {
    pub const R_RISCV_NONE: u32 = 0;
    pub const R_RISCV_RELATIVE: u32 = 3;
    pub const R_RISCV_64: u32 = 2;
}

/// AArch64 ELF relocation types.
#[allow(dead_code)]
mod aarch64_reloc_type {
    pub const R_AARCH64_NONE: u32 = 0;
    /// Copy-relocation with base+addend — the PIE equivalent of R_RISCV_RELATIVE.
    pub const R_AARCH64_RELATIVE: u32 = 1027; // 0x403
}

/// x86-64 ELF relocation types.
#[allow(dead_code)]
mod x86_64_reloc_type {
    pub const R_X86_64_NONE: u32 = 0;
    /// Copy-relocation with base+addend.
    pub const R_X86_64_RELATIVE: u32 = 8;
}

/// A single 64-bit RELA entry (Elf64_Rela layout, little-endian).
#[repr(C)]
#[derive(Copy, Clone)]
struct Rela64 {
    /// Offset within the ELF's virtual address space to patch.
    offset: u64,
    /// Relocation type + symbol index packed as (sym<<32 | type).
    info: u64,
    /// Addend to use in the relocation formula.
    addend: i64,
}

impl Rela64 {
    fn r_type(self) -> u32 {
        (self.info & 0xFFFF_FFFF) as u32
    }
}

/// Apply all relocations in the `.rela.dyn` section of an already-mapped ELF.
///
/// `base` is the load base chosen by the kernel. `owned_pages` is the exact
/// page set allocated for the new Cell; every word-sized write must stay within
/// one of those pages. For fixed-VA cells (non-PIE linker script), `base == 0`
/// and this function is a no-op beyond bounds checks.
///
/// # Errors
/// Returns `ViError::InvalidInput` on malformed or oversized relocation tables,
/// wrapping arithmetic, or a target outside the new Cell's owned pages.
/// Returns `ViError::NotSupported` if an unsupported relocation type is found.
pub fn apply_relocations(
    base: VAddr,
    owned_pages: &[crate::loader::elf::LoadedPage],
    rela_section: &[u8],
) -> ViResult<()> {
    let entry_size = core::mem::size_of::<Rela64>();
    if !rela_section.len().is_multiple_of(entry_size) {
        log::error!(
            "[reloc] .rela.dyn size {} not a multiple of Rela64 size {}",
            rela_section.len(),
            entry_size
        );
        return Err(ViError::InvalidInput);
    }

    let count = rela_section.len() / entry_size;
    if count > MAX_RELA_ENTRIES {
        log::error!(
            "[reloc] too many relocations: {} > {}",
            count,
            MAX_RELA_ENTRIES
        );
        return Err(ViError::InvalidInput);
    }

    for i in 0..count {
        let offset = i * entry_size;
        // SAFETY: bounds checked (offset + entry_size <= len).  Use
        // read_unaligned because the ELF buffer may not be 8-byte aligned —
        // the backing Vec is u8-aligned; section data inherits that.
        let entry: Rela64 = unsafe {
            core::ptr::read_unaligned(rela_section.as_ptr().add(offset) as *const Rela64)
        };

        macro_rules! apply_relative {
            () => {{
                let (patch_va, value) = crate::loader::reloc_target::relative_word(
                    base,
                    entry.offset,
                    entry.addend,
                    owned_pages,
                )?;

                // On AArch64 non-VHE (EL2 kernel), AP[1]=1 in the EL2 stage-1 PTE
                // means the page is read-only from EL2 — the same bit that grants EL0
                // access. Write through the physical frame's identity-mapped VA instead,
                // which is always kernel-writable (no AP[1]).
                // On RISC-V/x86 the SSTATUS.SUM / U-bit semantics allow S-mode writes.
                #[cfg(target_arch = "aarch64")]
                let write_ptr: *mut usize = {
                    let phys = crate::memory::paging::virt_to_phys(patch_va)
                        .expect("apply_relocations: owned relocation target unmapped");
                    crate::memory::frame::phys_to_virt(phys) as *mut usize
                };
                #[cfg(not(target_arch = "aarch64"))]
                let write_ptr: *mut usize = patch_va as *mut usize;

                // SAFETY: `relative_word` proved the full machine word remains
                // inside one page mapped exclusively for this new Cell.
                unsafe { write_ptr.write_unaligned(value) };
            }};
        }

        match entry.r_type() {
            // ── no-ops ────────────────────────────────────────────────────────
            // Relocation type 0 is universally "NONE" across every ELF psABI
            // (RISC-V, AArch64, x86-64 all define it as 0), so a single arm on
            // the shared literal value covers all three constants.
            riscv_reloc_type::R_RISCV_NONE => {}

            // ── RISC-V ────────────────────────────────────────────────────────
            riscv_reloc_type::R_RISCV_RELATIVE => {
                apply_relative!();
            }
            riscv_reloc_type::R_RISCV_64 => {
                // R_RISCV_64 with sym_index == 0 is an absolute-address fixup;
                // treat identically to R_RISCV_RELATIVE.
                let sym_index = (entry.info >> 32) as u32;
                if sym_index != 0 {
                    log::error!(
                        "[reloc] R_RISCV_64 with non-zero sym {} not supported",
                        sym_index
                    );
                    return Err(ViError::NotSupported);
                }
                apply_relative!();
            }

            // ── AArch64 ───────────────────────────────────────────────────────
            aarch64_reloc_type::R_AARCH64_RELATIVE => {
                apply_relative!();
            }

            // ── x86-64 ────────────────────────────────────────────────────────
            x86_64_reloc_type::R_X86_64_RELATIVE => {
                apply_relative!();
            }

            other => {
                log::error!("[reloc] unsupported relocation type {}", other);
                return Err(ViError::NotSupported);
            }
        }
    }

    log::debug!(
        "[reloc] applied {} relocation(s) with base 0x{:X}",
        count,
        base
    );
    Ok(())
}
