use super::{
    checked_mmio_end, claim_bdf_owner, owner_of_bdf, release_bdfs_for, static_range_allowed,
    valid_pcie_bar_window, DEV_GPIO, DEV_I2C, DEV_SPI,
};
use types::ViError;

const BCM_WINDOWS: &[(usize, usize, u8)] = &[
    (0x3F20_0000, 0x1000, DEV_GPIO),
    (0x3F20_4000, 0x1000, DEV_SPI),
    (0x3F80_4000, 0x1000, DEV_I2C),
];

#[test]
fn mmio_ranges_reject_zero_length_and_overflow() {
    assert_eq!(checked_mmio_end(0x1000, 0), Err(ViError::InvalidInput));
    assert_eq!(checked_mmio_end(usize::MAX, 2), Err(ViError::InvalidInput));
    assert_eq!(checked_mmio_end(0x1000, 0x1000), Ok(0x2000));
}

#[test]
fn controller_classes_only_authorize_their_own_window() {
    assert!(static_range_allowed(
        BCM_WINDOWS,
        0x3F80_4000,
        0x3F80_5000,
        DEV_I2C
    ));
    assert!(static_range_allowed(
        BCM_WINDOWS,
        0x3F20_4000,
        0x3F20_5000,
        DEV_SPI
    ));
    assert!(!static_range_allowed(
        BCM_WINDOWS,
        0x3F20_4000,
        0x3F20_5000,
        DEV_I2C
    ));
    assert!(!static_range_allowed(
        BCM_WINDOWS,
        0x3F80_4000,
        0x3F80_5000,
        DEV_SPI
    ));
}

#[test]
fn controller_grants_cannot_escape_or_reuse_gpio_authority() {
    assert!(!static_range_allowed(
        BCM_WINDOWS,
        0x3F80_4000,
        0x3F80_5001,
        DEV_I2C
    ));
    assert!(!static_range_allowed(
        BCM_WINDOWS,
        0x3F20_4000,
        0x3F20_5000,
        DEV_GPIO
    ));
}

#[test]
fn pcie_bar_windows_reject_unbounded_or_misaligned_authority() {
    assert!(valid_pcie_bar_window(0xF000_0000, 0x20_000));
    assert!(!valid_pcie_bar_window(0, 0x4000));
    assert!(!valid_pcie_bar_window(0xF000_0000, 0));
    assert!(!valid_pcie_bar_window(0xF000_1000, 0x4000));
    assert!(!valid_pcie_bar_window(0xF000_0000, 0x3000));
    assert!(!valid_pcie_bar_window(0x8000_0000, (1 << 30) + 1));
    assert!(!valid_pcie_bar_window(usize::MAX - 0xFFF, 0x1000));
}

#[test]
fn bdf_claim_preserves_live_owner_until_reap() {
    const BDF: u32 = 0x00FE_ED;
    const FIRST_TID: usize = 0xA001;
    const COMPETING_TID: usize = 0xA002;

    release_bdfs_for(FIRST_TID);
    release_bdfs_for(COMPETING_TID);
    assert!(claim_bdf_owner(BDF, FIRST_TID));
    assert!(claim_bdf_owner(BDF, FIRST_TID));
    assert!(!claim_bdf_owner(BDF, COMPETING_TID));
    assert_eq!(owner_of_bdf(BDF), Some(FIRST_TID));

    release_bdfs_for(FIRST_TID);
    assert!(claim_bdf_owner(BDF, COMPETING_TID));
    assert_eq!(owner_of_bdf(BDF), Some(COMPETING_TID));
    release_bdfs_for(COMPETING_TID);
}
