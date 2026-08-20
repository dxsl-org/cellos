use super::manifest::{
    CellManifest, MANIFEST_FLAG_I2C, MANIFEST_FLAG_SPI, MANIFEST_MAGIC, MANIFEST_VERSION,
    TIER_LEGACY,
};

#[test]
fn hardware_bus_flags_are_distinct_and_queryable() {
    let manifest = CellManifest::with_all(
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        true,
        false,
        TIER_LEGACY,
    );
    assert!(manifest.has_i2c());
    assert!(!manifest.has_spi());
    assert_eq!(MANIFEST_FLAG_I2C & MANIFEST_FLAG_SPI, 0);
}

#[test]
fn parser_preserves_i2c_and_spi_bits() {
    let flags = MANIFEST_FLAG_I2C | MANIFEST_FLAG_SPI;
    let mut bytes = [0u8; 16];
    bytes[0..4].copy_from_slice(&MANIFEST_MAGIC.to_le_bytes());
    bytes[4] = MANIFEST_VERSION;
    bytes[5] = TIER_LEGACY;
    bytes[6..8].copy_from_slice(&flags.to_le_bytes());
    let parsed = CellManifest::from_bytes(&bytes).expect("valid v2 manifest");
    assert!(parsed.has_i2c());
    assert!(parsed.has_spi());
}
