use super::manifest::*;

const ALL_FLAGS: [u16; 12] = [
    MANIFEST_FLAG_BLOCK_IO,
    MANIFEST_FLAG_NETWORK,
    MANIFEST_FLAG_SPAWN,
    MANIFEST_FLAG_GPIO,
    MANIFEST_FLAG_UART,
    MANIFEST_FLAG_HYPERVISOR,
    MANIFEST_FLAG_PART_DATA,
    MANIFEST_FLAG_PART_LFS,
    MANIFEST_FLAG_CAN,
    MANIFEST_FLAG_ADC,
    MANIFEST_FLAG_I2C,
    MANIFEST_FLAG_SPI,
];

fn v1(flags: u8) -> [u8; 8] {
    let magic = MANIFEST_MAGIC.to_le_bytes();
    [
        magic[0],
        magic[1],
        magic[2],
        magic[3],
        MANIFEST_VERSION_V1,
        flags,
        0,
        0,
    ]
}

fn v2(class: u8, flags: u16) -> [u8; 16] {
    let magic = MANIFEST_MAGIC.to_le_bytes();
    let flags = flags.to_le_bytes();
    [
        magic[0],
        magic[1],
        magic[2],
        magic[3],
        MANIFEST_VERSION,
        class,
        flags[0],
        flags[1],
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
    ]
}

#[test]
fn v2_layout_and_aliases_remain_byte_identical() {
    assert_eq!(core::mem::size_of::<CellManifest>(), 16);
    assert_eq!(core::mem::offset_of!(CellManifest, magic), 0);
    assert_eq!(core::mem::offset_of!(CellManifest, version), 4);
    assert_eq!(core::mem::offset_of!(CellManifest, tier), 5);
    assert_eq!(core::mem::offset_of!(CellManifest, flags), 6);
    assert_eq!(core::mem::offset_of!(CellManifest, cap_args_off), 8);
    assert_eq!(core::mem::offset_of!(CellManifest, reserved), 12);
    assert_eq!(
        v2(PROTECTION_CLASS_STANDARD, MANIFEST_FLAGS_MASK),
        [
            0x45, 0x43, 0x49, 0x56, 0x02, 0x01, 0xff, 0x0f, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00,
        ],
        "canonical v2 bytes are an ABI fixture",
    );
    let emitted = CellManifest::with_all(
        true,
        true,
        true,
        true,
        true,
        true,
        true,
        true,
        true,
        true,
        true,
        true,
        PROTECTION_CLASS_STANDARD,
    );
    assert_eq!(emitted.magic, MANIFEST_MAGIC);
    assert_eq!(emitted.version, MANIFEST_VERSION);
    assert_eq!(emitted.tier, PROTECTION_CLASS_STANDARD);
    assert_eq!(emitted.flags, MANIFEST_FLAGS_MASK);
    assert_eq!(emitted.cap_args_off, 0);
    assert_eq!(emitted.reserved, 0);

    for (class, alias) in [
        (PROTECTION_CLASS_TRUSTED_CORE, TIER_TRUSTED_CORE),
        (PROTECTION_CLASS_STANDARD, TIER_STANDARD),
        (PROTECTION_CLASS_FFI, TIER_TIER1B_FFI),
        (PROTECTION_CLASS_UNTRUSTED, TIER_UNTRUSTED),
        (PROTECTION_CLASS_LEGACY, TIER_LEGACY),
    ] {
        assert_eq!(class, alias);
        let parsed = CellManifest::from_bytes(&v2(class, MANIFEST_FLAGS_MASK)).unwrap();
        assert_eq!(parsed.tier(), alias);
        assert_eq!(parsed.protection_class(), class);
        assert_eq!(parsed.flags, MANIFEST_FLAGS_MASK);
    }
}

#[test]
fn every_flag_round_trips_in_its_supported_version() {
    for flag in ALL_FLAGS {
        let parsed = CellManifest::from_bytes(&v2(PROTECTION_CLASS_STANDARD, flag)).unwrap();
        assert_eq!(parsed.flags, flag);
    }
    for flag in ALL_FLAGS.into_iter().filter(|flag| *flag <= u8::MAX as u16) {
        let parsed = CellManifest::from_bytes(&v1(flag as u8)).unwrap();
        assert_eq!(parsed.flags, flag);
        assert_eq!(parsed.protection_class(), PROTECTION_CLASS_LEGACY);
        assert_eq!(parsed.version, MANIFEST_VERSION);
    }
}

#[test]
fn parser_requires_exact_versioned_record_boundaries() {
    let one = v1(MANIFEST_FLAG_BLOCK_IO as u8);
    let two = v2(PROTECTION_CLASS_STANDARD, MANIFEST_FLAG_SPI);
    for length in 0..=24 {
        let bytes = if length <= 8 {
            &one[..length]
        } else {
            let mut extended = std::vec::Vec::from(two);
            extended.resize(length, 0);
            assert_eq!(CellManifest::from_bytes(&extended).is_some(), length == 16);
            continue;
        };
        assert_eq!(CellManifest::from_bytes(bytes).is_some(), length == 8);
    }

    let mut padded_v1 = one.to_vec();
    padded_v1.push(0);
    assert!(CellManifest::from_bytes(&padded_v1).is_none());
    let mut nonzero_pad = one;
    nonzero_pad[7] = 1;
    assert!(CellManifest::from_bytes(&nonzero_pad).is_none());
}

#[test]
fn deterministic_mutation_corpus_is_bounded_and_panic_free() {
    let base = v2(PROTECTION_CLASS_STANDARD, MANIFEST_FLAGS_MASK);
    for byte in 0..base.len() {
        for bit in 0..8 {
            let mut mutation = base;
            mutation[byte] ^= 1 << bit;
            let _ = CellManifest::from_bytes(&mutation);
        }
    }

    for (index, replacement) in [(4, 0), (4, 3), (5, 4), (5, 0xfe), (8, 1), (15, 1)] {
        let mut mutation = base;
        mutation[index] = replacement;
        assert!(CellManifest::from_bytes(&mutation).is_none());
    }
    let mut unknown_flag = base;
    unknown_flag[7] |= 0x10;
    assert!(CellManifest::from_bytes(&unknown_flag).is_none());
}
