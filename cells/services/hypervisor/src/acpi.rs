//! Minimal ACPI tables for the x86 guest (RSDP → XSDT → FADT+DSDT + MADT).
//!
//! Alpine's `-virt` SMP kernel refuses to boot with `acpi=off` (verified: zero
//! output even under QEMU's own loader), so the VMM must present a valid ACPI
//! table set. This builds the smallest set Linux accepts: an RSDP (rev 2) that
//! the PVH `hvm_start_info.rsdp_paddr` points at, an XSDT listing the FADT and
//! MADT, a conventional (NOT Hardware-Reduced) FADT with zeroed PM blocks
//! pointing at an empty DSDT, and a MADT describing one Processor Local **APIC**
//! (xAPIC; the guest drives the LAPIC through the RAM-backed 0xFEE00000 MMIO
//! window) with the 8259 PIC still present (PCAT_COMPAT) for legacy IRQ lines.

extern crate alloc;
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
const LAPIC_ADDR: u32 = 0xFEE0_0000;
const MADT_PCAT_COMPAT: u32 = 1; // dual 8259 present

/// Build the ACPI blob. Returns `(bytes, rsdp_paddr)`; write `bytes` to
/// [`ACPI_BASE_GPA`] and pass `rsdp_paddr` in `hvm_start_info.rsdp_paddr`.
pub fn build() -> (Vec<u8>, u64) {
    let base = ACPI_BASE_GPA;
    let mut b = alloc::vec![0u8; OFF_MADT + 0x100];

    // ── DSDT (empty definition block) ─────────────────────────────────────────
    write_header(&mut b, OFF_DSDT, b"DSDT", 36, 2);
    finalize(&mut b, OFF_DSDT, 36);

    // ── FADT (conventional; points at DSDT) ───────────────────────────────────
    // NOT Hardware-Reduced: that flag makes Linux run acpi_reduced_hw_init(),
    // which sets legacy_pic = null_legacy_pic and timers.timer_init = noop —
    // the guest then never programs the 8259/8253 and the injected-PIT-tick
    // boot contract (see boot_x86 CMDLINE) can never fire. PM blocks and FACS
    // stay zero (SMI_CMD = 0 ⇒ ACPICA treats the system as already in ACPI
    // mode); SCI = IRQ9 keeps ACPI off the PIT's IRQ0 line.
    write_header(&mut b, OFF_FADT, b"FACP", FADT_LEN as u32, 6);
    w32(&mut b, OFF_FADT + 40, (base + OFF_DSDT as u64) as u32); // DSDT (32-bit)
    b[OFF_FADT + 46] = 9; // SCI_INT (u16 LE, high byte stays 0)
    w64(&mut b, OFF_FADT + 140, base + OFF_DSDT as u64); // X_DSDT (64-bit)
    finalize(&mut b, OFF_FADT, FADT_LEN);

    // ── MADT (one Processor Local APIC; PIC present) ──────────────────────────
    // Type-0 (xAPIC), not type-9 (x2APIC): the guest drives the LAPIC via the
    // RAM-backed 0xFEE00000 MMIO window (x2APIC would need IRQ remapping the VMM
    // does not emulate). No IOAPIC entry → the guest keeps the 8259 in
    // virtual-wire mode, and the boot tick arrives as an injected PIT IRQ0.
    let madt_len = 44 + 8; // header + one Processor-Local-APIC entry
    write_header(&mut b, OFF_MADT, b"APIC", madt_len as u32, 5);
    w32(&mut b, OFF_MADT + 36, LAPIC_ADDR);
    w32(&mut b, OFF_MADT + 40, MADT_PCAT_COMPAT);
    // Entry: Processor Local APIC (type 0, len 8).
    let e = OFF_MADT + 44;
    b[e] = 0;
    b[e + 1] = 8;
    b[e + 2] = 0; // acpi_processor_uid
    b[e + 3] = 0; // apic_id = 0
    w32(&mut b, e + 4, 1); // flags = enabled
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
