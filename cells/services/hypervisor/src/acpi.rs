//! Minimal ACPI tables for the x86 guest (RSDP → XSDT → FADT+DSDT + MADT).
//!
//! Alpine's `-virt` SMP kernel refuses to boot with `acpi=off` (verified: zero
//! output even under QEMU's own loader), so the VMM must present a valid ACPI
//! table set. This builds the smallest set Linux accepts: an RSDP (rev 2) that
//! the PVH `hvm_start_info.rsdp_paddr` points at, an XSDT listing the FADT and
//! MADT, a conventional (NOT Hardware-Reduced) FADT naming the PM1a blocks
//! (`acpi_pm.rs`) and a DSDT that holds only `\_S5`, so that `poweroff` ends the
//! VM, and a MADT describing one Processor Local **APIC** per vCPU (xAPIC; the
//! guest drives the LAPIC through the 0xFEE00000 MMIO window) with the 8259 PIC
//! still present (PCAT_COMPAT) for legacy IRQ lines.

extern crate alloc;
use crate::acpi_pm::{PM1A_CNT_BLK, PM1A_EVT_BLK, PM1_CNT_LEN, PM1_EVT_LEN};
use alloc::vec::Vec;

/// GPA where the ACPI blob is placed (low RAM, below the 640 KiB line, clear of
/// the boot-info blob at 0x1000).
pub const ACPI_BASE_GPA: u64 = 0x8000;

// Sub-table offsets within the blob.
const OFF_RSDP: usize = 0x000;
const OFF_XSDT: usize = 0x040;
const OFF_FADT: usize = 0x080;
const OFF_DSDT: usize = 0x1C0;
const OFF_MADT: usize = 0x200;

const FADT_LEN: usize = 276; // ACPI 6.x FADT
/// AML of `Name (\_S5, Package (4) {0, 0, 0, 0})`: sleep type 0 is S5, the one
/// sleep state the platform has.
const DSDT_S5: [u8; 12] = [
    0x08, b'_', b'S', b'5', b'_', 0x12, 0x06, 0x04, 0x00, 0x00, 0x00, 0x00,
];
const DSDT_LEN: usize = 36 + DSDT_S5.len();
const LAPIC_ADDR: u32 = 0xFEE0_0000;
const MADT_PCAT_COMPAT: u32 = 1; // dual 8259 present
/// Processor entries that fit the MADT's slot in the blob.
pub const MAX_CPUS: u8 = 16;

/// Build the ACPI blob for `cpus` vCPUs (APIC IDs 0..cpus, clamped to
/// 1..=[`MAX_CPUS`]). Returns `(bytes, rsdp_paddr)`; write `bytes` to
/// [`ACPI_BASE_GPA`] and pass `rsdp_paddr` in `hvm_start_info.rsdp_paddr`.
pub fn build(cpus: u8) -> (Vec<u8>, u64) {
    let base = ACPI_BASE_GPA;
    let mut b = alloc::vec![0u8; OFF_MADT + 0x100];

    // ── DSDT (`\_S5` only) ────────────────────────────────────────────────────
    write_header(&mut b, OFF_DSDT, b"DSDT", DSDT_LEN as u32, 2);
    b[OFF_DSDT + 36..OFF_DSDT + DSDT_LEN].copy_from_slice(&DSDT_S5);
    finalize(&mut b, OFF_DSDT, DSDT_LEN);

    // ── FADT (conventional; points at DSDT) ───────────────────────────────────
    // NOT Hardware-Reduced: that flag makes Linux run acpi_reduced_hw_init(),
    // which sets legacy_pic = null_legacy_pic and timers.timer_init = noop —
    // the guest then never programs the 8259/8253 and the injected-PIT-tick
    // boot contract (see boot_x86 CMDLINE) can never fire. FACS stays zero
    // (SMI_CMD = 0 ⇒ ACPICA treats the system as already in ACPI mode); SCI =
    // IRQ9 keeps ACPI off the PIT's IRQ0 line. The PM1a blocks are legacy
    // 32-bit fields only; ACPICA derives the extended ones from them.
    write_header(&mut b, OFF_FADT, b"FACP", FADT_LEN as u32, 6);
    w32(&mut b, OFF_FADT + 40, (base + OFF_DSDT as u64) as u32); // DSDT (32-bit)
    b[OFF_FADT + 46] = 9; // SCI_INT (u16 LE, high byte stays 0)
    w32(&mut b, OFF_FADT + 56, u32::from(PM1A_EVT_BLK));
    w32(&mut b, OFF_FADT + 64, u32::from(PM1A_CNT_BLK));
    b[OFF_FADT + 88] = PM1_EVT_LEN;
    b[OFF_FADT + 89] = PM1_CNT_LEN;
    w64(&mut b, OFF_FADT + 140, base + OFF_DSDT as u64); // X_DSDT (64-bit)
    finalize(&mut b, OFF_FADT, FADT_LEN);

    // ── MADT (one Processor Local APIC per vCPU; PIC present) ─────────────────
    // Type-0 (xAPIC), not type-9 (x2APIC): the guest drives the LAPIC via the
    // 0xFEE00000 MMIO window (x2APIC would need IRQ remapping the VMM does not
    // emulate). No IOAPIC entry → the guest keeps the 8259 in virtual-wire
    // mode on the first processor, and the boot tick arrives as an injected
    // PIT IRQ0.
    let cpus = cpus.clamp(1, MAX_CPUS);
    let madt_len = 44 + 8 * usize::from(cpus); // header + Processor-Local-APIC entries
    write_header(&mut b, OFF_MADT, b"APIC", madt_len as u32, 5);
    w32(&mut b, OFF_MADT + 36, LAPIC_ADDR);
    w32(&mut b, OFF_MADT + 40, MADT_PCAT_COMPAT);
    for cpu in 0..cpus {
        // Entry: Processor Local APIC (type 0, len 8); the APIC ID is the
        // vCPU's index, as the kernel numbers them.
        let e = OFF_MADT + 44 + 8 * usize::from(cpu);
        b[e] = 0;
        b[e + 1] = 8;
        b[e + 2] = cpu; // acpi_processor_uid
        b[e + 3] = cpu; // apic_id
        w32(&mut b, e + 4, 1); // flags = enabled
    }
    finalize(&mut b, OFF_MADT, madt_len);

    // ── XSDT (points at FADT + MADT) ──────────────────────────────────────────
    let xsdt_len = 36 + 16; // header + two 64-bit entries
    write_header(&mut b, OFF_XSDT, b"XSDT", xsdt_len as u32, 1);
    w64(&mut b, OFF_XSDT + 36, base + OFF_FADT as u64);
    w64(&mut b, OFF_XSDT + 44, base + OFF_MADT as u64);
    finalize(&mut b, OFF_XSDT, xsdt_len);

    // ── RSDP (rev 2 → XSDT) ───────────────────────────────────────────────────
    b[OFF_RSDP..OFF_RSDP + 8].copy_from_slice(b"RSD PTR ");
    b[OFF_RSDP + 9..OFF_RSDP + 15].copy_from_slice(b"VICELL");
    b[OFF_RSDP + 15] = 2; // revision
    w32(&mut b, OFF_RSDP + 16, (base + OFF_XSDT as u64) as u32); // RsdtAddress (unused)
    w32(&mut b, OFF_RSDP + 20, 36); // length
    w64(&mut b, OFF_RSDP + 24, base + OFF_XSDT as u64); // XsdtAddress
    b[OFF_RSDP + 8] = checksum(&b[OFF_RSDP..OFF_RSDP + 20]); // v1 checksum
    b[OFF_RSDP + 32] = checksum(&b[OFF_RSDP..OFF_RSDP + 36]); // extended checksum

    (b, base)
}

/// Write a standard 36-byte ACPI table header (signature, length, revision) and
/// a fixed OEM id; the checksum is filled by [`finalize`].
fn write_header(b: &mut [u8], off: usize, sig: &[u8; 4], length: u32, revision: u8) {
    b[off..off + 4].copy_from_slice(sig);
    w32(b, off + 4, length);
    b[off + 8] = revision;
    b[off + 10..off + 16].copy_from_slice(b"VICELL");
    b[off + 16..off + 24].copy_from_slice(b"VICELLOS");
    w32(b, off + 24, 1); // oem_revision
    b[off + 28..off + 32].copy_from_slice(b"VICL"); // creator id
    w32(b, off + 32, 1); // creator_revision
}

/// Zero then set the header checksum so the whole table sums to 0 (mod 256).
fn finalize(b: &mut [u8], off: usize, len: usize) {
    b[off + 9] = 0;
    b[off + 9] = checksum(&b[off..off + len]);
}

fn checksum(bytes: &[u8]) -> u8 {
    (0u8).wrapping_sub(bytes.iter().fold(0u8, |a, &x| a.wrapping_add(x)))
}

#[inline]
fn w32(b: &mut [u8], off: usize, v: u32) {
    b[off..off + 4].copy_from_slice(&v.to_le_bytes());
}

#[inline]
fn w64(b: &mut [u8], off: usize, v: u64) {
    b[off..off + 8].copy_from_slice(&v.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(blob: &[u8], off: usize) -> &[u8] {
        let len = u32::from_le_bytes(blob[off + 4..off + 8].try_into().unwrap()) as usize;
        &blob[off..off + len]
    }

    #[test]
    fn tables_checksum_and_offer_s5_through_the_pm1a_control_block() {
        let (blob, _) = build(1);
        for off in [OFF_XSDT, OFF_FADT, OFF_DSDT, OFF_MADT] {
            let sum = table(&blob, off)
                .iter()
                .fold(0u8, |a, &x| a.wrapping_add(x));
            assert_eq!(sum, 0, "table at {off:#x}");
        }
        assert!(table(&blob, OFF_DSDT).ends_with(&DSDT_S5));
        let fadt = table(&blob, OFF_FADT);
        let pm1a_cnt = u32::from_le_bytes(fadt[64..68].try_into().unwrap());
        assert_eq!((pm1a_cnt, fadt[89]), (u32::from(PM1A_CNT_BLK), PM1_CNT_LEN));
    }

    #[test]
    fn madt_names_one_enabled_local_apic_per_vcpu_and_fits_its_slot() {
        for (asked, listed) in [(0, 1), (4, 4), (MAX_CPUS, MAX_CPUS), (200, MAX_CPUS)] {
            let (blob, _) = build(asked);
            let madt = table(&blob, OFF_MADT);
            assert_eq!(madt.iter().fold(0u8, |a, &x| a.wrapping_add(x)), 0);
            assert!(OFF_MADT + madt.len() <= blob.len());
            let entries: Vec<&[u8]> = madt[44..].chunks(8).collect();
            assert_eq!(entries.len(), usize::from(listed));
            for (id, entry) in entries.iter().enumerate() {
                assert_eq!(entry[..4], [0, 8, id as u8, id as u8]);
                assert_eq!(entry[4..], [1, 0, 0, 0]);
            }
        }
    }
}
