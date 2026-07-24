//! PVH `hvm_start_info` boot-info blob builder (x86 guest).
//!
//! PVH enters the guest in 32-bit protected mode with `%ebx` pointing at an
//! `hvm_start_info` structure (Xen `start_info.h`). This assembles that struct
//! plus its satellite tables — an e820-style memory map and a one-entry module
//! list for the initramfs — and the kernel command line into one contiguous
//! blob placed in low guest RAM. `acpi=off` ⇒ `rsdp_paddr = 0` (no ACPI tables).

extern crate alloc;
use alloc::vec::Vec;

/// GPA where the boot-info blob is placed (below the 1 MiB kernel load area,
/// clear of the low-640 KiB RAM the guest may use).
pub const BOOT_INFO_GPA: u64 = 0x1000;

const HVM_START_MAGIC: u32 = 0x336E_C578;

// Sub-table offsets within the blob (start_info is 56 B; leave slack).
const OFF_MEMMAP: usize = 0x40;
const OFF_MODLIST: usize = 0x100;
const OFF_CMDLINE: usize = 0x180;

const E820_RAM: u32 = 1;

/// Inputs describing the guest RAM + loaded initramfs.
pub struct BootInfoParams<'a> {
    pub ram_size: u64,
    pub initrd_gpa: u64,
    pub initrd_size: u64,
    pub cmdline: &'a str,
    /// GPA of the ACPI RSDP (0 = none). Alpine's kernel requires ACPI.
    pub rsdp_paddr: u64,
}

/// Build the boot-info blob. Returns `(bytes, start_info_gpa)`; write `bytes`
/// to `start_info_gpa` (= [`BOOT_INFO_GPA`]) and pass that GPA to the guest in
/// RBX/EBX.
pub fn build(p: &BootInfoParams) -> (Vec<u8>, u64) {
    let base = BOOT_INFO_GPA;

    // e820 map: 0..640 KiB usable, then 1 MiB..RAM end usable. The 0xA0000..
    // 0xFFFFF legacy hole is left unlisted (reserved), as on real hardware.
    let mut memmap: Vec<(u64, u64, u32)> = Vec::new();
    memmap.push((0x0, 0xA_0000, E820_RAM));
    if p.ram_size > 0x10_0000 {
        memmap.push((0x10_0000, p.ram_size - 0x10_0000, E820_RAM));
    }

    let mut blob = alloc::vec![0u8; OFF_CMDLINE];

    // ── hvm_start_info @ 0 ────────────────────────────────────────────────────
    w32(&mut blob, 0x00, HVM_START_MAGIC);
    w32(&mut blob, 0x04, 1); // version
    w32(&mut blob, 0x08, 0); // flags
    w32(&mut blob, 0x0C, 1); // nr_modules (initramfs)
    w64(&mut blob, 0x10, base + OFF_MODLIST as u64); // modlist_paddr
    w64(&mut blob, 0x18, base + OFF_CMDLINE as u64); // cmdline_paddr
    w64(&mut blob, 0x20, p.rsdp_paddr); // rsdp_paddr
    w64(&mut blob, 0x28, base + OFF_MEMMAP as u64); // memmap_paddr
    w32(&mut blob, 0x30, memmap.len() as u32); // memmap_entries
    w32(&mut blob, 0x34, 0); // reserved

    // ── hvm_memmap_table_entry[] @ OFF_MEMMAP (24 B each) ─────────────────────
    for (i, (addr, size, ty)) in memmap.iter().enumerate() {
        let o = OFF_MEMMAP + i * 24;
        w64(&mut blob, o, *addr);
        w64(&mut blob, o + 8, *size);
        w32(&mut blob, o + 16, *ty);
        w32(&mut blob, o + 20, 0);
    }

    // ── hvm_modlist_entry @ OFF_MODLIST (initramfs; 32 B) ─────────────────────
    w64(&mut blob, OFF_MODLIST, p.initrd_gpa);
    w64(&mut blob, OFF_MODLIST + 8, p.initrd_size);
    w64(&mut blob, OFF_MODLIST + 16, 0); // cmdline_paddr (module has none)
    w64(&mut blob, OFF_MODLIST + 24, 0); // reserved

    // ── command line @ OFF_CMDLINE (NUL-terminated) ───────────────────────────
    blob.extend_from_slice(p.cmdline.as_bytes());
    blob.push(0);

    (blob, base)
}

#[inline]
fn w32(buf: &mut [u8], off: usize, val: u32) {
    buf[off..off + 4].copy_from_slice(&val.to_le_bytes());
}

#[inline]
fn w64(buf: &mut [u8], off: usize, val: u64) {
    buf[off..off + 8].copy_from_slice(&val.to_le_bytes());
}
