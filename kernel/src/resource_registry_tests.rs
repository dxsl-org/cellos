use super::{checked_mmio_end, static_range_allowed, DEV_GPIO, DEV_I2C, DEV_SPI};
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
