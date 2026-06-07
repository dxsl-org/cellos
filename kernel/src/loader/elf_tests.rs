//! Boot-time tests for the ELF loader and relocation engine.
//!
//! Functions are `pub` so `kernel/src/loader.rs` can invoke them from the
//! boot-time test runner.  Every function logs with `log::info!` and asserts
//! with standard `assert!`/`assert_eq!` — a failing assert panics the kernel,
//! which is intentional (hard failure = detected early).

use api::manifest::{
    CellManifest, MANIFEST_FLAG_BLOCK_IO, MANIFEST_FLAG_NETWORK, MANIFEST_FLAGS_MASK,
    MANIFEST_MAGIC, MANIFEST_VERSION,
};
use api::syscall::ViSyscall;
use types::{ViError, ViResult};

/// Run all ELF + relocation tests and log a summary.
pub fn run_all() {
    log::info!("=== ELF Loader Tests ===");
    test_spawn_path_empty_rejected();
    test_spawn_path_no_leading_slash_rejected();
    test_spawn_path_too_long_rejected();
    test_spawn_path_valid_format_accepted();
    test_reloc_empty_section_ok();
    test_reloc_non_multiple_size_rejected();
    test_reloc_too_many_entries_rejected();
    test_reloc_unsupported_type_rejected();
    test_reloc_none_type_noop();
    test_reloc_relative_patches_memory();
    test_manifest_size_is_8();
    test_manifest_parsing_valid();
    test_manifest_parsing_bad_magic();
    test_manifest_parsing_short();
    test_manifest_parsing_bad_version();
    test_manifest_reserved_flags_rejected();
    test_manifest_network_false_grants_no_network_cap();
    test_force_exit_opcode_mapped();
    test_force_exit_allowlist_bit_none();
    log::info!("=== ELF Loader Tests PASSED ===");
}

// ─── spawn_from_path path validation ─────────────────────────────────────────

fn expect_invalid(res: ViResult<usize>, label: &str) {
    match res {
        Err(ViError::InvalidInput) | Err(ViError::NotFound) => {}
        other => panic!("expected error for {}, got {:?}", label, other),
    }
}

fn test_spawn_path_empty_rejected() {
    let res = crate::loader::spawn_from_path("");
    expect_invalid(res, "empty path");
    log::info!("  [ok] empty path rejected");
}

fn test_spawn_path_no_leading_slash_rejected() {
    let res = crate::loader::spawn_from_path("bin/shell");
    expect_invalid(res, "no leading slash");
    log::info!("  [ok] path without leading '/' rejected");
}

fn test_spawn_path_too_long_rejected() {
    let long: alloc::string::String = "/".repeat(300);
    let res = crate::loader::spawn_from_path(&long);
    expect_invalid(res, "path too long");
    log::info!("  [ok] path longer than MAX_CELL_PATH rejected");
}

fn test_spawn_path_valid_format_accepted() {
    // A well-formatted path may still fail with NotFound (disk not ready) —
    // that is acceptable; only InvalidInput counts as a format rejection.
    let res = crate::loader::spawn_from_path("/bin/nonexistent-elf-for-test");
    match res {
        Err(ViError::NotFound) | Ok(_) => {}
        Err(ViError::InvalidInput) => panic!("well-formed path should not be rejected as InvalidInput"),
        Err(e) => {
            log::warn!("  [ok] /bin/nonexistent-elf-for-test → {:?} (expected NotFound)", e);
        }
    }
    log::info!("  [ok] well-formed path passes format validation");
}

// ─── apply_relocations ───────────────────────────────────────────────────────

/// Construct the 24-byte raw encoding of a single Rela64 entry.
/// Layout (LE): offset:u64, info:u64, addend:i64
fn make_rela(offset: u64, r_type: u32, addend: i64) -> [u8; 24] {
    let mut b = [0u8; 24];
    b[0..8].copy_from_slice(&offset.to_le_bytes());
    b[8..16].copy_from_slice(&(r_type as u64).to_le_bytes()); // sym=0 in high 32 bits
    b[16..24].copy_from_slice(&(addend as u64).to_le_bytes());
    b
}

fn test_reloc_empty_section_ok() {
    let res = crate::loader::reloc::apply_relocations(0, &[]);
    assert!(res.is_ok(), "empty section should succeed: {:?}", res);
    log::info!("  [ok] empty .rela.dyn → Ok");
}

fn test_reloc_non_multiple_size_rejected() {
    // 7 bytes is not a multiple of 24 (sizeof Rela64).
    let bad = [0u8; 7];
    let res = crate::loader::reloc::apply_relocations(0, &bad);
    assert_eq!(res, Err(ViError::InvalidInput), "non-multiple size should be InvalidInput");
    log::info!("  [ok] non-multiple rela size → InvalidInput");
}

fn test_reloc_too_many_entries_rejected() {
    // 65_537 * 24 bytes > MAX_RELA_ENTRIES limit.
    const OVER_LIMIT: usize = 65_537;
    let big = alloc::vec![0u8; OVER_LIMIT * 24];
    let res = crate::loader::reloc::apply_relocations(0, &big);
    assert_eq!(res, Err(ViError::InvalidInput), "over-limit count should be InvalidInput");
    log::info!("  [ok] {} entries (> 65536) → InvalidInput", OVER_LIMIT);
}

fn test_reloc_unsupported_type_rejected() {
    // Type 0xFF is not a recognised RISC-V relocation.
    let entry = make_rela(0, 0xFF, 0);
    let res = crate::loader::reloc::apply_relocations(0, &entry);
    assert_eq!(res, Err(ViError::NotSupported), "unknown type should be NotSupported");
    log::info!("  [ok] unknown reloc type 0xFF → NotSupported");
}

fn test_reloc_none_type_noop() {
    // R_RISCV_NONE (type=0) must be silently skipped.
    let entry = make_rela(0, 0, 0); // type=0
    let res = crate::loader::reloc::apply_relocations(0, &entry);
    assert!(res.is_ok(), "R_RISCV_NONE should be a no-op: {:?}", res);
    log::info!("  [ok] R_RISCV_NONE → no-op, Ok");
}

fn test_reloc_relative_patches_memory() {
    // Allocate a writable buffer; use its address as `base`.
    // Create R_RISCV_RELATIVE (type=3): offset=0, addend=0x400.
    // Expected result: *(base + 0) = base + 0x400.
    let mut buf = alloc::vec![0u8; 64];
    let base = buf.as_mut_ptr() as usize;

    let entry = make_rela(0, 3, 0x400_i64); // R_RISCV_RELATIVE
    let res = crate::loader::reloc::apply_relocations(base, &entry);
    assert!(res.is_ok(), "R_RISCV_RELATIVE should succeed: {:?}", res);

    // Read back the patched value (usize-width, unaligned-safe).
    // SAFETY: buf is alive for the duration of this test; we wrote exactly
    // sizeof(usize) bytes at offset 0 via apply_relocations.
    let patched: usize = unsafe {
        core::ptr::read_unaligned(buf.as_ptr() as *const usize)
    };
    let expected = base.wrapping_add(0x400);
    assert_eq!(patched, expected, "R_RISCV_RELATIVE patch value mismatch");
    log::info!("  [ok] R_RISCV_RELATIVE patched 0x{:X} → 0x{:X}", base, expected);
}

// ─── CellManifest parsing ────────────────────────────────────────────────────

fn manifest_bytes(magic: u32, version: u8, flags: u8) -> [u8; 8] {
    let m = magic.to_le_bytes();
    [m[0], m[1], m[2], m[3], version, flags, 0, 0]
}

fn test_manifest_size_is_8() {
    assert_eq!(
        core::mem::size_of::<CellManifest>(), 8,
        "CellManifest must be exactly 8 bytes (ABI invariant)"
    );
    log::info!("  [ok] CellManifest is 8 bytes");
}

fn test_manifest_parsing_valid() {
    let bytes = manifest_bytes(
        MANIFEST_MAGIC, MANIFEST_VERSION,
        MANIFEST_FLAG_BLOCK_IO | MANIFEST_FLAG_NETWORK,
    );
    let m = CellManifest::from_bytes(&bytes).expect("valid manifest must parse");
    assert!(m.has_block_io(), "block_io flag must be set");
    assert!(m.has_network(),  "network flag must be set");
    assert!(!m.has_spawn(),   "spawn flag must be clear");
    assert!(m.declares_any_privilege(), "declares_any_privilege must be true");
    log::info!("  [ok] valid manifest parses with correct flags");
}

fn test_manifest_parsing_bad_magic() {
    let bytes = manifest_bytes(0xDEAD_BEEF, MANIFEST_VERSION, 0);
    assert!(
        CellManifest::from_bytes(&bytes).is_none(),
        "wrong magic must return None"
    );
    log::info!("  [ok] bad magic rejected");
}

fn test_manifest_parsing_short() {
    assert!(
        CellManifest::from_bytes(&[0u8; 4]).is_none(),
        "slice shorter than 8 bytes must return None"
    );
    log::info!("  [ok] short slice rejected");
}

fn test_manifest_parsing_bad_version() {
    let bytes = manifest_bytes(MANIFEST_MAGIC, MANIFEST_VERSION.wrapping_add(1), 0);
    assert!(
        CellManifest::from_bytes(&bytes).is_none(),
        "unsupported version must return None"
    );
    log::info!("  [ok] bad version rejected");
}

fn test_manifest_reserved_flags_rejected() {
    // Any bit above the defined mask must be rejected — prevents stale v1 binaries
    // from silently gaining a future-version capability via a reserved bit.
    let reserved = !MANIFEST_FLAGS_MASK; // e.g., 0xF8
    let bytes = manifest_bytes(MANIFEST_MAGIC, MANIFEST_VERSION, reserved | 0x01);
    assert!(
        CellManifest::from_bytes(&bytes).is_none(),
        "reserved flags must return None"
    );
    // Pure reserved (no defined flags): also rejected.
    let bytes2 = manifest_bytes(MANIFEST_MAGIC, MANIFEST_VERSION, 0x08);
    assert!(
        CellManifest::from_bytes(&bytes2).is_none(),
        "reserved-only flags must return None"
    );
    log::info!("  [ok] reserved flag bits rejected");
}

fn test_force_exit_opcode_mapped() {
    // Opcode 61 must resolve to ForceExit; any other result means the dispatcher
    // would silently ignore ForceExit calls.
    assert!(matches!(ViSyscall::from(61), ViSyscall::ForceExit),
        "opcode 61 must resolve to ViSyscall::ForceExit");
    log::info!("  [ok] opcode 61 → ForceExit");
}

fn test_force_exit_allowlist_bit_none() {
    // ForceExit must bypass the allowlist (like Exit/Yield); SpawnCap is the gate.
    assert!(ViSyscall::ForceExit.allowlist_bit().is_none(),
        "ForceExit must not have an allowlist bit — SpawnCap is the authority check");
    log::info!("  [ok] ForceExit allowlist_bit = None");
}

fn test_manifest_network_false_grants_no_network_cap() {
    let m = CellManifest::new(true, false, false, false, false, false);
    assert!(m.has_block_io(),   "block_io=true must set block_io flag");
    assert!(!m.has_network(),   "network=false must NOT set network flag");
    assert!(!m.has_spawn(),     "spawn=false must NOT set spawn flag");
    assert!(m.declares_any_privilege(), "block_io alone is still a privilege");
    log::info!("  [ok] network=false → no NetworkCap granted");
}
