use super::manifest::{
    CellManifest, MANIFEST_FLAG_I2C, MANIFEST_FLAG_SPI, MANIFEST_MAGIC, MANIFEST_VERSION,
    PROTECTION_CLASS_FFI, PROTECTION_CLASS_LEGACY, PROTECTION_CLASS_STANDARD,
    PROTECTION_CLASS_TRUSTED_CORE, PROTECTION_CLASS_UNTRUSTED, TIER_LEGACY, TIER_STANDARD,
    TIER_TIER1B_FFI, TIER_TRUSTED_CORE, TIER_UNTRUSTED,
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
        PROTECTION_CLASS_LEGACY,
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
    bytes[5] = PROTECTION_CLASS_LEGACY;
    bytes[6..8].copy_from_slice(&flags.to_le_bytes());
    let parsed = CellManifest::from_bytes(&bytes).expect("valid v2 manifest");
    assert!(parsed.has_i2c());
    assert!(parsed.has_spi());
}

#[test]
fn protection_class_aliases_match_legacy_tier_constants() {
    assert_eq!(PROTECTION_CLASS_TRUSTED_CORE, TIER_TRUSTED_CORE);
    assert_eq!(PROTECTION_CLASS_STANDARD, TIER_STANDARD);
    assert_eq!(PROTECTION_CLASS_FFI, TIER_TIER1B_FFI);
    assert_eq!(PROTECTION_CLASS_UNTRUSTED, TIER_UNTRUSTED);
    assert_eq!(PROTECTION_CLASS_LEGACY, TIER_LEGACY);
}

#[test]
fn protection_class_accessor_matches_tier_accessor_and_layout() {
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
        false,
        false,
        PROTECTION_CLASS_FFI,
    );
    assert_eq!(manifest.protection_class(), manifest.tier());
    assert_eq!(manifest.protection_class(), TIER_TIER1B_FFI);
    assert_eq!(core::mem::size_of::<CellManifest>(), 16);
}
