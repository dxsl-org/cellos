use scsi_block::{sense_key, Sense, SenseClass};

fn fixed(code: u8, key: u8, asc: u8, ascq: u8) -> [u8; 18] {
    let mut data = [0u8; 18];
    data[0] = code;
    data[2] = key;
    data[7] = 10;
    data[12] = asc;
    data[13] = ascq;
    data
}

#[test]
fn fixed_format_current_and_deferred() {
    let expected = Sense {
        key: sense_key::MEDIUM_ERROR,
        asc: 0x11,
        ascq: 0x00,
    };
    assert_eq!(Sense::decode(&fixed(0x70, 0x03, 0x11, 0)), Some(expected));
    assert_eq!(Sense::decode(&fixed(0x71, 0x03, 0x11, 0)), Some(expected));
    // VALID bit set, and FILEMARK/EOM/ILI bits above the key nibble.
    assert_eq!(Sense::decode(&fixed(0xF0, 0xE3, 0x11, 0)), Some(expected));
}

#[test]
fn fixed_format_without_asc_coverage() {
    let mut data = fixed(0x70, sense_key::UNIT_ATTENTION, 0x29, 0x00);
    data[7] = 5;
    let short = Sense {
        key: sense_key::UNIT_ATTENTION,
        asc: 0,
        ascq: 0,
    };
    assert_eq!(Sense::decode(&data), Some(short));
    assert_eq!(
        Sense::decode(&fixed(0x70, 0x06, 0x29, 0)[..13]),
        Some(short)
    );
    assert_eq!(Sense::decode(&[0x70, 0]), None);
}

#[test]
fn descriptor_format() {
    let expected = Sense {
        key: sense_key::ILLEGAL_REQUEST,
        asc: 0x20,
        ascq: 0x00,
    };
    assert_eq!(
        Sense::decode(&[0x72, 0x05, 0x20, 0x00, 0, 0, 0, 0]),
        Some(expected)
    );
    assert_eq!(Sense::decode(&[0x73, 0x05, 0x20, 0x00]), Some(expected));
    assert_eq!(
        Sense::decode(&[0x72, 0x02]),
        Some(Sense {
            key: sense_key::NOT_READY,
            asc: 0,
            ascq: 0
        })
    );
    assert_eq!(Sense::decode(&[0x72]), None);
}

#[test]
fn unknown_response_code_and_empty() {
    assert_eq!(Sense::decode(&fixed(0x7F, 0x03, 0x11, 0)), None);
    assert_eq!(Sense::decode(&[]), None);
}

#[test]
fn retry_classification() {
    let class = |key, asc, ascq| Sense { key, asc, ascq }.class();
    assert_eq!(
        class(sense_key::UNIT_ATTENTION, 0x29, 0x00),
        SenseClass::Retry
    );
    assert_eq!(class(sense_key::NOT_READY, 0x04, 0x01), SenseClass::Retry);
    // NOT READY for other reasons: no medium, manual intervention.
    assert_eq!(class(sense_key::NOT_READY, 0x3A, 0x00), SenseClass::Fatal);
    assert_eq!(class(sense_key::NOT_READY, 0x04, 0x03), SenseClass::Fatal);
    for key in [
        sense_key::MEDIUM_ERROR,
        sense_key::HARDWARE_ERROR,
        sense_key::ILLEGAL_REQUEST,
        sense_key::DATA_PROTECT,
        sense_key::ABORTED_COMMAND,
    ] {
        assert_eq!(class(key, 0x04, 0x01), SenseClass::Fatal, "key {key:#x}");
    }
}
