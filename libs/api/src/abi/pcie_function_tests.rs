// SPDX-License-Identifier: Apache-2.0
//! Host tests for `ViPcieFunctionV1` and the `AllocPcieIrq` result.

use super::*;

fn sample() -> ViPcieFunctionV1 {
    let mut rec = ViPcieFunctionV1 {
        cap_count: 2,
        flags: FLAG_MSIX_PRESENT | FLAG_MSI_PRESENT,
        bdf: 0x0018,
        vendor_id: 0x1af4,
        device_id: 0x1041,
        subsystem_vendor_id: 0x1af4,
        subsystem_id: 0x0001,
        class: 0x02,
        subclass: 0x00,
        prog_if: 0x00,
        revision: 0x01,
        msix: PcieMsix {
            table_size: 3,
            table_bar: 1,
            pba_bar: 1,
            table_offset: 0,
            pba_offset: 0x800,
        },
        msi_control: 0x0080,
        ..ViPcieFunctionV1::default()
    };
    rec.bars[1] = PcieBar {
        base: 0xfebd_1000,
        len: 0x1000,
        kind: BAR_KIND_MEM32,
        bar_flags: BAR_FLAG_MSIX_CARVED,
    };
    rec.bars[4] = PcieBar {
        base: 0xfe00_0000,
        len: 0x4000,
        kind: BAR_KIND_MEM64,
        bar_flags: BAR_FLAG_PREFETCHABLE,
    };
    rec.caps[0] = PcieCap {
        id: 0x11,
        offset: 0x98,
        len: 12,
        bytes: [
            0x11, 0x84, 0x02, 0, 0x01, 0, 0, 0, 0x01, 0x08, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ],
    };
    rec.caps[1] = PcieCap {
        id: 0x09,
        offset: 0x84,
        len: 20,
        bytes: [
            0x09, 0x70, 0x14, 0x02, 4, 0, 0, 0, 0, 0x30, 0, 0, 0, 0x10, 0, 0, 4, 0, 0, 0,
        ],
    };
    rec
}

#[test]
fn field_offsets_match_cp1_layout() {
    let bytes = sample().encode();
    assert_eq!(bytes.len(), 584);
    assert_eq!(&bytes[0..2], &[1, 0]);
    assert_eq!(&bytes[2..4], &[2, 0]);
    assert_eq!(&bytes[8..12], &[0x18, 0, 0, 0]);
    assert_eq!(&bytes[12..14], &[0xf4, 0x1a]);
    assert_eq!(bytes[20], 0x02);
    assert_eq!(bytes[23], 0x01);
    // bars[1] at 32 + 24: base, len, kind, flags.
    assert_eq!(&bytes[56..64], &0xfebd_1000u64.to_le_bytes());
    assert_eq!(&bytes[72..76], &BAR_KIND_MEM32.to_le_bytes());
    assert_eq!(&bytes[76..80], &BAR_FLAG_MSIX_CARVED.to_le_bytes());
    assert_eq!(&bytes[176..178], &[3, 0]);
    assert_eq!(&bytes[184..188], &0x800u32.to_le_bytes());
    assert_eq!(&bytes[192..194], &[0x80, 0]);
    // caps[1] at 200 + 24: id, offset, len, reserved, bytes.
    assert_eq!(&bytes[224..228], &[0x09, 0x84, 20, 0]);
    assert_eq!(bytes[247], 0);
    assert!(bytes[248..].iter().all(|&b| b == 0));
}

#[test]
fn decode_round_trips() {
    let rec = sample();
    assert_eq!(ViPcieFunctionV1::decode(&rec.encode()), Some(rec));
    assert_eq!(rec.cap(0x09).map(|c| c.offset), Some(0x84));
    assert!(rec.cap(0x05).is_none());
}

#[test]
fn encode_zeroes_entries_past_count_and_bytes_past_len() {
    let mut rec = sample();
    rec.caps[5].id = 0x10;
    rec.caps[0].bytes[19] = 0xAA;
    let bytes = rec.encode();
    assert!(bytes[200 + 5 * 24..200 + 6 * 24].iter().all(|&b| b == 0));
    assert_eq!(bytes[200 + 4 + 19], 0);
}

#[test]
fn decode_refuses_malformed_records() {
    let good = sample().encode();
    let corrupt = |at: usize, value: u8| {
        let mut bytes = good;
        bytes[at] = value;
        ViPcieFunctionV1::decode(&bytes)
    };
    assert!(ViPcieFunctionV1::decode(&good[..583]).is_none());
    assert!(corrupt(0, 2).is_none(), "version");
    assert!(corrupt(2, 17).is_none(), "cap_count");
    assert!(corrupt(4, 0x08).is_none(), "unknown flag");
    assert!(corrupt(10, 1).is_none(), "bdf above 16 bits");
    assert!(corrupt(24, 1).is_none(), "reserved0");
    assert!(corrupt(32 + 16, 4).is_none(), "bar kind");
    assert!(corrupt(32 + 20, 4).is_none(), "bar flag");
    assert!(corrupt(188, 1).is_none(), "msix reserved");
    assert!(corrupt(196, 1).is_none(), "msi reserved");
    assert!(corrupt(203, 1).is_none(), "cap reserved");
    assert!(corrupt(202, 21).is_none(), "cap len");
    assert!(corrupt(200 + 4 + 12, 1).is_none(), "byte past cap len");
    assert!(
        corrupt(200 + 2 * 24, 0x10).is_none(),
        "entry past cap_count"
    );
}

#[test]
fn irq_alloc_result_packs_and_refuses_garbage() {
    let alloc = PcieIrqAlloc {
        first_irq: 0x31,
        count: 4,
        kind: IRQ_KIND_MSI,
    };
    assert_eq!(alloc.pack(), 0x02_04_31);
    assert_eq!(PcieIrqAlloc::unpack(alloc.pack()), Some(alloc));
    assert_eq!(PcieIrqAlloc::unpack(0x01_00_31), None, "count 0");
    assert_eq!(PcieIrqAlloc::unpack(0x03_01_31), None, "kind");
    assert_eq!(PcieIrqAlloc::unpack(0x0101_01_31), None, "high bits");
}
