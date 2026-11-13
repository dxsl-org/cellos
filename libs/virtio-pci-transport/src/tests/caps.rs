// SPDX-License-Identifier: Apache-2.0 OR MIT

use super::fake::{qemu_caps, record, record_with, vcap, BAR, MULTIPLIER, NOTIFY};
use crate::{parse_caps, CapRegion, CfgType, TransportError};
use abi::pcie_function::{BAR_KIND_IO, PCIE_FUNCTION_CAPS_MAX};

fn region(bar: u8, offset: u32, length: u32) -> CapRegion {
    CapRegion {
        bar,
        offset,
        length,
    }
}

#[test]
fn parses_all_four_cfg_types_and_skips_other_caps() {
    let caps = parse_caps(&record()).unwrap();
    assert_eq!(caps.common, region(BAR, 0, 0x1000));
    assert_eq!(caps.isr, region(BAR, 0x1000, 0x1000));
    assert_eq!(caps.device, Some(region(BAR, 0x2000, 0x1000)));
    assert_eq!(caps.notify, region(BAR, NOTIFY as u32, 0x1000));
    assert_eq!(caps.notify_off_multiplier, MULTIPLIER);
}

#[test]
fn first_cap_of_each_type_wins() {
    let mut caps = qemu_caps().to_vec();
    caps.push(vcap(1, BAR, 0x800, 0x100, None));
    caps.push(vcap(2, BAR, 0x3800, 0x800, Some(8)));
    let parsed = parse_caps(&record_with(&caps)).unwrap();
    assert_eq!(parsed.common, region(BAR, 0, 0x1000));
    assert_eq!(parsed.notify, region(BAR, NOTIFY as u32, 0x1000));
    assert_eq!(parsed.notify_off_multiplier, MULTIPLIER);
}

#[test]
fn first_cap_wins_even_when_it_is_invalid() {
    let mut caps = qemu_caps();
    caps[2] = vcap(3, BAR, 0x4000, 1, None);
    let mut caps = caps.to_vec();
    caps.push(vcap(3, BAR, 0x1000, 1, None));
    let err = parse_caps(&record_with(&caps)).unwrap_err();
    assert_eq!(err, TransportError::RegionOutOfBar(CfgType::Isr));
}

#[test]
fn region_outside_bar_refused() {
    let mut caps = qemu_caps();
    caps[3] = vcap(4, BAR, 0x3f00, 0x101, None);
    let err = parse_caps(&record_with(&caps)).unwrap_err();
    assert_eq!(err, TransportError::RegionOutOfBar(CfgType::Device));
}

#[test]
fn region_offset_overflowing_u32_refused() {
    let mut caps = qemu_caps();
    caps[1] = vcap(1, BAR, u32::MAX, 0x1000, None);
    let err = parse_caps(&record_with(&caps)).unwrap_err();
    assert_eq!(err, TransportError::RegionOutOfBar(CfgType::Common));
}

#[test]
fn io_and_absent_bars_refused() {
    let mut rec = record();
    rec.bars[BAR as usize].kind = BAR_KIND_IO;
    assert_eq!(parse_caps(&rec), Err(TransportError::NotMemoryBar(BAR)));
    let mut caps = qemu_caps();
    caps[1] = vcap(1, 5, 0, 0x100, None);
    let err = parse_caps(&record_with(&caps)).unwrap_err();
    assert_eq!(err, TransportError::NotMemoryBar(5));
}

#[test]
fn unallocated_bar_refused() {
    let mut rec = record();
    rec.bars[BAR as usize].base = 0;
    assert_eq!(parse_caps(&rec), Err(TransportError::BarNotAllocated(BAR)));
}

#[test]
fn short_notify_cap_skipped() {
    let mut short = vcap(2, BAR, NOTIFY as u32, 0x1000, Some(MULTIPLIER));
    short.len = 16;
    let mut caps = qemu_caps();
    caps[4] = short;
    let err = parse_caps(&record_with(&caps)).unwrap_err();
    assert_eq!(err, TransportError::MissingCap(CfgType::Notify));

    let mut caps = caps.to_vec();
    caps.push(vcap(2, BAR, 0x3800, 0x800, Some(2)));
    let parsed = parse_caps(&record_with(&caps)).unwrap();
    assert_eq!(parsed.notify, region(BAR, 0x3800, 0x800));
    assert_eq!(parsed.notify_off_multiplier, 2);
}

#[test]
fn reserved_bar_index_and_truncated_caps_skipped() {
    let mut caps = qemu_caps().to_vec();
    let mut truncated = vcap(1, BAR, 0x800, 0x100, None);
    truncated.len = 15;
    caps.insert(1, truncated);
    caps.insert(1, vcap(1, 6, 0, 0x100, None));
    let parsed = parse_caps(&record_with(&caps)).unwrap();
    assert_eq!(parsed.common, region(BAR, 0, 0x1000));
}

#[test]
fn odd_notify_multiplier_refused() {
    let mut caps = qemu_caps();
    caps[4] = vcap(2, BAR, NOTIFY as u32, 0x1000, Some(3));
    let err = parse_caps(&record_with(&caps)).unwrap_err();
    assert_eq!(err, TransportError::InvalidNotifyOffMultiplier(3));
}

#[test]
fn legacy_only_refused() {
    let caps = &qemu_caps()[..1];
    assert_eq!(
        parse_caps(&record_with(caps)),
        Err(TransportError::LegacyOnly)
    );
}

#[test]
fn missing_isr_refused_and_device_cfg_optional() {
    let caps = qemu_caps();
    let err = parse_caps(&record_with(&[caps[1], caps[3], caps[4]])).unwrap_err();
    assert_eq!(err, TransportError::MissingCap(CfgType::Isr));
    let parsed = parse_caps(&record_with(&[caps[1], caps[2], caps[4]])).unwrap();
    assert_eq!(parsed.device, None);
}

#[test]
fn common_region_shorter_than_registers_refused() {
    let mut caps = qemu_caps();
    caps[1] = vcap(1, BAR, 0, 55, None);
    let err = parse_caps(&record_with(&caps)).unwrap_err();
    assert_eq!(err, TransportError::RegionTooSmall(CfgType::Common));
}

#[test]
fn caps_past_cap_count_ignored() {
    let mut rec = record();
    rec.caps[PCIE_FUNCTION_CAPS_MAX - 1] = vcap(1, BAR, 0x800, 0x100, None);
    rec.cap_count = 1;
    assert_eq!(parse_caps(&rec), Err(TransportError::LegacyOnly));
}
