//! Host tests for `proto.rs`: LUN and request encoding, and completion
//! decoding, including malformed input.

use super::*;
use scsi_block::sense_key;

#[test]
fn luns_are_single_level_below_256() {
    assert_eq!(device_lun([0; 8]), Ok([0; 8]));
    assert_eq!(
        device_lun([0, 5, 0, 0, 0, 0, 0, 0]),
        Ok([0, 5, 0, 0, 0, 0, 0, 0])
    );
    assert_eq!(
        device_lun([0x40, 7, 0, 0, 0, 0, 0, 0]),
        Ok([0, 7, 0, 0, 0, 0, 0, 0])
    );
    for bad in [
        [0x41, 0, 0, 0, 0, 0, 0, 0],
        [0, 1, 0, 2, 0, 0, 0, 0],
        [0x80, 1, 0, 0, 0, 0, 0, 0],
    ] {
        assert_eq!(device_lun(bad), Err(PvscsiError::UnsupportedLun(bad)));
    }
}

fn request(cdb: &[u8], to_host: Option<bool>, data_len: usize) -> Request<'_> {
    Request {
        context: 0x1122_3344_5566_7788,
        target: 3,
        lun: [0, 1, 0, 0, 0, 0, 0, 0],
        cdb,
        to_host,
        data_addr: 0x4000,
        data_len,
        sense_addr: 0x3000,
    }
}

#[test]
fn request_encoding_places_every_field() {
    let d = encode_request(&request(
        &[0x28, 0, 0, 0, 0, 9, 0, 0, 1, 0],
        Some(true),
        512,
    ))
    .unwrap();
    assert_eq!(d[..8], 0x1122_3344_5566_7788u64.to_le_bytes());
    assert_eq!(d[8..16], 0x4000u64.to_le_bytes());
    assert_eq!(d[16..24], 512u64.to_le_bytes());
    assert_eq!(d[24..32], 0x3000u64.to_le_bytes());
    assert_eq!(d[32..36], (SENSE_LEN as u32).to_le_bytes());
    assert_eq!(d[36..40], (1u32 << 3).to_le_bytes(), "device to host");
    assert_eq!(d[40..50], [0x28, 0, 0, 0, 0, 9, 0, 0, 1, 0]);
    assert_eq!(
        (d[56], &d[57..65], d[65], d[66], d[67]),
        (10, &[0, 1, 0, 0, 0, 0, 0, 0][..], 0x20, 0, 3)
    );
    assert!(d[68..].iter().all(|&b| b == 0));

    let write = encode_request(&request(&[0x2A; 10], Some(false), 512)).unwrap();
    assert_eq!(write[36..40], (1u32 << 4).to_le_bytes(), "host to device");
    let none = encode_request(&request(&[0x00; 6], Some(true), 0)).unwrap();
    assert_eq!(none[36..40], (1u32 << 2).to_le_bytes(), "no data phase");
    assert_eq!(none[8..24], [0; 16], "no data address or length");
    assert_eq!(
        encode_request(&request(&[0; 17], None, 0)),
        Err(PvscsiError::CdbTooLong(17))
    );
}

fn completion(context: u64, data_len: u64, sense_len: u32, host: u16, scsi: u16) -> [u8; CMP_LEN] {
    let mut c = [0u8; CMP_LEN];
    c[..8].copy_from_slice(&context.to_le_bytes());
    c[8..16].copy_from_slice(&data_len.to_le_bytes());
    c[16..20].copy_from_slice(&sense_len.to_le_bytes());
    c[20..22].copy_from_slice(&host.to_le_bytes());
    c[22..24].copy_from_slice(&scsi.to_le_bytes());
    c
}

#[test]
fn completions_decode_status_residual_and_sense() {
    let sense = {
        let mut s = [0u8; SENSE_LEN];
        s[..14].copy_from_slice(&[
            0x70,
            0,
            sense_key::UNIT_ATTENTION,
            0,
            0,
            0,
            0,
            6,
            0,
            0,
            0,
            0,
            0x29,
            0,
        ]);
        s
    };
    let ok = decode_completion(&completion(9, 0, 0, 0, 0), 9, 512, &sense).unwrap();
    assert_eq!((ok.status, ok.sense, ok.residual), (status::GOOD, None, 0));
    let short = decode_completion(&completion(9, 16, 0, 0x12, 0), 9, 264, &sense).unwrap();
    assert_eq!(short.residual, 248, "DATARUN with a reported length");
    let unknown = decode_completion(&completion(9, 0, 0, 0x12, 0), 9, 264, &sense).unwrap();
    assert_eq!(unknown.residual, 0, "QEMU leaves data_len 0");
    let check = decode_completion(&completion(9, 0, 18, 0, 2), 9, 0, &sense).unwrap();
    assert_eq!(check.status, status::CHECK_CONDITION);
    assert_eq!(check.sense.map(|s| s.key), Some(sense_key::UNIT_ATTENTION));
    let oversized_sense = decode_completion(&completion(9, 0, 4096, 0, 2), 9, 0, &sense).unwrap();
    assert!(
        oversized_sense.sense.is_some(),
        "sense length is clamped, not trusted"
    );
}

#[test]
fn malformed_completions_are_errors_not_panics() {
    let sense = [0u8; SENSE_LEN];
    let cases = [
        (completion(8, 0, 0, 0, 0), PvscsiError::BadCompletion), // foreign context
        (completion(9, 4097, 0, 0x12, 0), PvscsiError::BadCompletion), // overrun
        (completion(9, 0, 0, 0, 0x100), PvscsiError::BadCompletion), // status > u8
        (completion(9, 0, 0, 0x11, 0), PvscsiError::NoTarget),
        (completion(9, 0, 0, 0x30, 0), PvscsiError::HostStatus(0x30)),
    ];
    for (cmp, expected) in cases {
        assert_eq!(decode_completion(&cmp, 9, 4096, &sense), Err(expected));
    }
    assert!(PvscsiError::BadCompletion.poisons() && PvscsiError::Timeout.poisons());
    assert!(!PvscsiError::NoTarget.poisons() && PvscsiError::NoTarget.issued());
    assert!(!PvscsiError::CdbTooLong(17).issued() && !PvscsiError::RingFull.issued());
    // Every byte pattern decodes without panicking.
    for fill in [0x00, 0x7F, 0xFF] {
        let _ = decode_completion(
            &[fill; CMP_LEN],
            u64::from_le_bytes([fill; 8]),
            4096,
            &[fill; 3],
        );
    }
}
