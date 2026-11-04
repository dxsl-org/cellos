mod common;

use common::{capacity_data, inquiry_data, lun, report_luns_data};
use scsi_block::parse::{self, Capacity, Inquiry};
use scsi_block::ParseError;

#[test]
fn inquiry_disk_and_non_disk() {
    let disk = parse::inquiry(&inquiry_data(0, 0)).unwrap();
    assert_eq!(
        disk,
        Inquiry {
            qualifier: 0,
            device_type: 0
        }
    );
    assert!(disk.is_disk());
    assert!(!parse::inquiry(&inquiry_data(0, 5)).unwrap().is_disk());
    // Qualifier 1: supported but not connected; 3: no LUN here.
    assert!(!parse::inquiry(&inquiry_data(1, 0)).unwrap().is_disk());
    assert_eq!(
        parse::inquiry(&inquiry_data(3, 0x1F)).unwrap(),
        Inquiry {
            qualifier: 3,
            device_type: 0x1F
        }
    );
}

#[test]
fn inquiry_short_buffer() {
    assert_eq!(parse::inquiry(&[0; 4]), Err(ParseError::InquiryTooShort(4)));
    assert!(parse::inquiry(&[0; 5]).is_ok());
}

#[test]
fn report_luns_lists_entries() {
    let data = report_luns_data(&[lun(0), lun(2)]);
    let list = parse::report_luns(&data).unwrap();
    assert_eq!(list.len(), 2);
    assert!(!list.truncated());
    assert_eq!(list.iter().collect::<Vec<_>>(), vec![lun(0), lun(2)]);
}

#[test]
fn report_luns_truncated_by_buffer() {
    let data = report_luns_data(&[lun(0), lun(1), lun(2)]);
    // Only the header, one full entry, and half of the next arrived.
    let list = parse::report_luns(&data[..20]).unwrap();
    assert_eq!(list.iter().collect::<Vec<_>>(), vec![lun(0)]);
    assert!(list.truncated());
}

#[test]
fn report_luns_bounded_by_list_length() {
    let mut data = report_luns_data(&[lun(4)]);
    data.extend_from_slice(&lun(9)); // stale bytes past LUN LIST LENGTH
    let list = parse::report_luns(&data).unwrap();
    assert_eq!(list.iter().collect::<Vec<_>>(), vec![lun(4)]);
    assert!(!list.truncated());
}

#[test]
fn report_luns_partial_list_length_rounds_down() {
    let mut data = report_luns_data(&[lun(1), lun(2)]);
    data[3] = 12;
    let list = parse::report_luns(&data).unwrap();
    assert_eq!(list.iter().collect::<Vec<_>>(), vec![lun(1)]);
}

#[test]
fn report_luns_empty_and_short() {
    assert!(parse::report_luns(&report_luns_data(&[]))
        .unwrap()
        .is_empty());
    assert_eq!(
        parse::report_luns(&[0; 7]),
        Err(ParseError::ReportLunsTooShort(7))
    );
}

#[test]
fn read_capacity_16_parses() {
    assert_eq!(
        parse::read_capacity_16(&capacity_data(0x1_0000_0000, 4096)).unwrap(),
        Capacity {
            block_count: 0x1_0000_0000,
            block_size: 4096
        }
    );
    // The 12 mandatory bytes are enough.
    assert_eq!(
        parse::read_capacity_16(&capacity_data(8, 512)[..12]).unwrap(),
        Capacity {
            block_count: 8,
            block_size: 512
        }
    );
}

#[test]
fn read_capacity_16_short_buffer() {
    assert_eq!(
        parse::read_capacity_16(&[0; 11]),
        Err(ParseError::CapacityTooShort(11))
    );
}

#[test]
fn read_capacity_16_rejects_bad_block_size() {
    for size in [0, 1, 256, 500, 520, 1000, 4097] {
        assert_eq!(
            parse::read_capacity_16(&capacity_data(8, size)),
            Err(ParseError::BadBlockSize(size)),
            "block size {size}"
        );
    }
}

#[test]
fn read_capacity_16_rejects_lba_overflow() {
    let mut data = capacity_data(1, 512);
    data[..8].copy_from_slice(&u64::MAX.to_be_bytes());
    assert_eq!(
        parse::read_capacity_16(&data),
        Err(ParseError::CapacityOverflow)
    );
}
