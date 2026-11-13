//! Layer-2 hardware security self-tests.
//!
//! Runs at boot when the `test-hooks` feature is active. Each test prints
//! `{FEATURE}-SELFTEST: PASS` or `{FEATURE}-SELFTEST: SKIP` (never FAIL for
//! expected "feature absent" — only FAIL for feature present but broken).
//!
//! ⚠️ Never ship `test-hooks` in a production image. These tests exercise
//! raw hardware state at boot and emit extra serial output that changes the
//! boot log structure CI greps against.
//!
//! # Preconditions
//! Call after all hardware init but before the scheduler starts. The tests
//! must run in Ring-0 / EL1 (kernel mode) with interrupts still masked.

// ─── AArch64 MTE self-test ─────────────────────────────────────────────────

/// AArch64 MTE tag round-trip self-test.
///
/// Tags a 32-byte static buffer with colour 0x5, verifies both granules read
/// back as 0x5, re-tags with colour 0xA, and verifies the new tag.
///
/// # SKIP conditions
/// - MTE2 not present in ID_AA64PFR1_EL1[11:8].
///
/// # FAIL conditions
/// - Tag round-trip returns wrong value (MTE asm broken or SCTLR not set).
#[cfg(all(target_arch = "aarch64", feature = "test-hooks"))]
pub fn run_mte_selftest() {
    // Read ID_AA64PFR1_EL1 — the MTE field is [11:8].
    // ≥ 2 means MTE2 (full load+store tag checking) is available.
    let pfr1: u64;
    // SAFETY: mrs from a read-only ID register; no memory side effects.
    unsafe {
        core::arch::asm!(
            "mrs {}, id_aa64pfr1_el1",
            out(reg) pfr1,
            options(nomem, nostack)
        );
    }
    if ((pfr1 >> 8) & 0xF) < 2 {
        puts_aa64("[SELFTEST] MTE-SELFTEST: SKIP: MTE2 unavailable\n");
        return;
    }

    // 32-byte test buffer — two MTE granules (16 bytes each).
    // `static mut` is safe here: single-threaded boot code, no other Rust
    // reference exists, and the buffer is only written via raw asm (STG).
    static mut MTE_BUF: [u8; 32] = [0u8; 32];
    // addr_of_mut! is safe — no reference to the static is created.
    let ptr = core::ptr::addr_of_mut!(MTE_BUF) as *mut u8;

    // Tag both granules with colour 0x5.
    // SAFETY: ptr is 16-byte aligned (static alignment), len is a multiple of
    // 16, and the region is valid Normal-Tagged kernel memory.
    unsafe {
        tag_region(ptr, 32, 0x5);
    }

    // SAFETY: ptr points into the freshly tagged region.
    let tag0 = unsafe { get_tag(ptr) };
    // SAFETY: ptr+16 is within the 32-byte MTE_BUF (granule 2).
    let tag1 = unsafe { get_tag(ptr.add(16)) };
    if tag0 != 0x5 || tag1 != 0x5 {
        puts_aa64("[SELFTEST] MTE-SELFTEST: FAIL: tag round-trip (0x5) failed\n");
        return;
    }

    // Re-tag with colour 0xA (simulates allocation-tag change on "free").
    // SAFETY: same constraints as above.
    unsafe {
        tag_region(ptr, 32, 0xA);
    }
    let new_tag = unsafe { get_tag(ptr) };
    if new_tag != 0xA {
        puts_aa64("[SELFTEST] MTE-SELFTEST: FAIL: re-tag (0xA) failed\n");
        return;
    }

    puts_aa64("[SELFTEST] MTE-SELFTEST: PASS\n");
}

/// Tag `len` bytes starting at `ptr` with MTE colour `color`.
///
/// Uses STG (Store Tag) to write the allocation tag for each 16-byte granule.
/// Requires MTE2+ and ATA/ATA0 set in SCTLR_EL1 (done by `hal::mte::init()`).
///
/// # Safety
/// - `ptr` must be 16-byte aligned.
/// - `len` must be a non-zero multiple of 16.
/// - The range `[ptr, ptr + len)` must be valid kernel memory (Normal-Tagged).
/// - Must be called from EL1 with interrupts masked (boot path).
#[cfg(all(target_arch = "aarch64", feature = "test-hooks"))]
// `stg` is rejected by the assembler unless MTE is enabled. Scoping the feature
// to this function keeps the rest of the kernel's codegen untouched, and is
// sound because the caller returns early unless ID_AA64PFR1_EL1 reports MTE2.
#[target_feature(enable = "mte")]
unsafe fn tag_region(ptr: *mut u8, len: usize, color: u8) {
    let tag = (color & 0xF) as u64;
    let mut cur = ptr as u64;
    let end = cur + len as u64;
    while cur < end {
        // Bits [59:56] carry the allocation tag; TBI strips the top byte at
        // load/store so normal memory accesses are unaffected.
        let tagged_ptr = (cur & !(0xFu64 << 56)) | (tag << 56);
        // SAFETY: STG touches only the tag memory for the 16-byte granule at
        // `tagged_ptr`. The caller guarantees the range is valid.
        unsafe {
            core::arch::asm!(
                "stg {p}, [{p}]",
                p = in(reg) tagged_ptr,
                options(nostack)
            );
        }
        cur += 16;
    }
}

/// Return the 4-bit MTE allocation tag for the granule at `ptr`.
///
/// Uses LDG (Load Tag) which places the tag in bits [59:56] of the output.
///
/// # Safety
/// - `ptr` must point inside a valid Normal-Tagged region that was previously
///   tagged with [`tag_region`].
#[cfg(all(target_arch = "aarch64", feature = "test-hooks"))]
// `ldg` needs the MTE feature for the same reason `tag_region` does; the same
// availability check guards the caller.
#[target_feature(enable = "mte")]
unsafe fn get_tag(ptr: *const u8) -> u8 {
    let mut result: u64 = ptr as u64;
    // SAFETY: LDG reads only the tag memory for the granule at `ptr`.
    unsafe {
        core::arch::asm!(
            "ldg {r}, [{r}]",
            r = inout(reg) result,
            options(nostack)
        );
    }
    ((result >> 56) & 0xF) as u8
}

/// Write `s` to the PL011 UART (aarch64 boot path).
#[cfg(all(target_arch = "aarch64", feature = "test-hooks"))]
fn puts_aa64(s: &str) {
    for c in s.bytes() {
        crate::hal::uart_pl011::putchar(c);
    }
}
