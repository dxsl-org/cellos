use super::*;

#[test]
fn generic_pc_preserves_the_verified_legacy_contract() {
    assert_eq!(GENERIC_PC.validate(), Ok(()));
    assert_eq!(GENERIC_PC.com1.base, 0x03F8);
    assert_eq!(GENERIC_PC.com1.irq, 4);
    assert!(GENERIC_PC.legacy_bios_window.contains(0x0008_0000, 16));
    assert!(!GENERIC_PC.legacy_bios_window.contains(0x0007_FFFF, 16));
    assert!(GENERIC_PC.legacy_rsdp_window.contains(0x000E_0000, 36));
}

#[test]
fn firmware_windows_reject_overflow_and_out_of_range_access() {
    let overflowing = AddressRange {
        base: usize::MAX - 1,
        size: 4,
    };
    assert_eq!(overflowing.end(), None);
    assert!(!overflowing.contains(usize::MAX - 1, 1));
    assert!(!GENERIC_PC.legacy_rsdp_window.contains(0x000F_FFF0, 32));
}
