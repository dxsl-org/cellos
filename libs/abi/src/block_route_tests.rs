// SPDX-License-Identifier: Apache-2.0
//! Host tests for the `ViBlockRouteV1` record and `DeviceKey`.

use super::*;

fn live() -> ViBlockRouteV1 {
    ViBlockRouteV1 {
        version: BLOCK_ROUTE_VERSION,
        kind: BLOCK_DEVICE_PCI,
        flags: FLAG_PROVIDER_LIVE,
        id: 0x00FA,
        provider_tid: 42,
        cell_id: 0x1122_3344_5566_7788,
        generation: 9,
    }
}

fn idle() -> ViBlockRouteV1 {
    ViBlockRouteV1 {
        kind: BLOCK_DEVICE_MMIO,
        flags: 0,
        id: 0x1000_1000,
        provider_tid: 0,
        cell_id: 0,
        generation: 0,
        ..live()
    }
}

#[test]
fn layout_is_forty_bytes_at_declared_offsets() {
    assert_eq!(core::mem::size_of::<ViBlockRouteV1>(), 40);
    assert_eq!(core::mem::offset_of!(ViBlockRouteV1, kind), 2);
    assert_eq!(core::mem::offset_of!(ViBlockRouteV1, flags), 4);
    assert_eq!(core::mem::offset_of!(ViBlockRouteV1, id), 8);
    assert_eq!(core::mem::offset_of!(ViBlockRouteV1, provider_tid), 16);
    assert_eq!(core::mem::offset_of!(ViBlockRouteV1, cell_id), 24);
    assert_eq!(core::mem::offset_of!(ViBlockRouteV1, generation), 32);
    let bytes = live().encode();
    assert_eq!(&bytes[0..2], &1u16.to_le_bytes());
    assert_eq!(&bytes[2..4], &1u16.to_le_bytes());
    assert_eq!(&bytes[4..8], &1u32.to_le_bytes());
    assert_eq!(&bytes[8..16], &0xFAu64.to_le_bytes());
    assert_eq!(&bytes[16..24], &42u64.to_le_bytes());
    assert_eq!(&bytes[24..32], &0x1122_3344_5566_7788u64.to_le_bytes());
    assert_eq!(&bytes[32..40], &9u64.to_le_bytes());
}

#[test]
fn round_trip_live_and_idle() {
    for record in [live(), idle()] {
        assert_eq!(ViBlockRouteV1::decode(&record.encode()), Ok(record));
    }
    assert_eq!(live().provider(), Some((42, 0x1122_3344_5566_7788, 9)));
    assert_eq!(idle().provider(), None);
    assert_eq!(
        idle().device(),
        DeviceKey {
            kind: BLOCK_DEVICE_MMIO,
            id: 0x1000_1000
        }
    );
}

#[test]
fn length_other_than_exact_is_refused() {
    let bytes = live().encode();
    assert_eq!(
        ViBlockRouteV1::decode(&bytes[..39]),
        Err(RouteRecordError::Length)
    );
    let mut long = [0u8; 41];
    long[..40].copy_from_slice(&bytes);
    assert_eq!(ViBlockRouteV1::decode(&long), Err(RouteRecordError::Length));
}

#[test]
fn unknown_version_kind_and_flags_are_refused() {
    for version in [0u16, 2, u16::MAX] {
        let r = ViBlockRouteV1 { version, ..live() };
        assert_eq!(
            ViBlockRouteV1::decode(&r.encode()),
            Err(RouteRecordError::UnsupportedVersion)
        );
    }
    for kind in [0u16, 3, u16::MAX] {
        let r = ViBlockRouteV1 { kind, ..live() };
        assert_eq!(
            ViBlockRouteV1::decode(&r.encode()),
            Err(RouteRecordError::Device)
        );
    }
    for bit in 1..32 {
        let r = ViBlockRouteV1 {
            flags: FLAG_PROVIDER_LIVE | 1 << bit,
            ..live()
        };
        assert_eq!(
            ViBlockRouteV1::decode(&r.encode()),
            Err(RouteRecordError::UnknownFlag)
        );
    }
}

#[test]
fn device_ids_out_of_range_are_refused() {
    let pci = ViBlockRouteV1 {
        id: 0x1_0000,
        ..live()
    };
    assert_eq!(
        ViBlockRouteV1::decode(&pci.encode()),
        Err(RouteRecordError::Device)
    );
    let mmio = ViBlockRouteV1 { id: 0, ..idle() };
    assert_eq!(
        ViBlockRouteV1::decode(&mmio.encode()),
        Err(RouteRecordError::Device)
    );
    assert_eq!(
        DeviceKey::new(BLOCK_DEVICE_PCI, 0xFFFF),
        Some(DeviceKey {
            kind: 1,
            id: 0xFFFF
        })
    );
    assert_eq!(
        DeviceKey::new(BLOCK_DEVICE_PCI, 0),
        Some(DeviceKey { kind: 1, id: 0 })
    );
    assert_eq!(DeviceKey::new(BLOCK_DEVICE_MMIO, 0), None);
    assert_eq!(DeviceKey::new(0, 1), None);
}

#[test]
fn provider_fields_must_match_liveness() {
    let dead_tid = ViBlockRouteV1 {
        provider_tid: 0,
        ..live()
    };
    assert_eq!(
        ViBlockRouteV1::decode(&dead_tid.encode()),
        Err(RouteRecordError::Liveness)
    );
    for stale in [
        ViBlockRouteV1 {
            provider_tid: 5,
            ..idle()
        },
        ViBlockRouteV1 {
            cell_id: 5,
            ..idle()
        },
        ViBlockRouteV1 {
            generation: 5,
            ..idle()
        },
    ] {
        assert_eq!(
            ViBlockRouteV1::decode(&stale.encode()),
            Err(RouteRecordError::Liveness)
        );
    }
}
