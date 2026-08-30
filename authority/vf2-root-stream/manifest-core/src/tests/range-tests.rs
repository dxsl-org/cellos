use crate::*;

fn values() -> (Manifest, ExpectedManifest, ManifestLimits) {
    let kinds = [
        ComponentKind::OpenSbi,
        ComponentKind::Dtb,
        ComponentKind::Cellos,
        ComponentKind::Vifs,
    ];
    let addresses = [0x8000_0000, 0x8100_0000, 0x8200_0000, 0x8300_0000];
    let components = core::array::from_fn(|i| Component {
        kind: kinds[i],
        offset: i as u64 * 16,
        length: 16,
        load_address: addresses[i],
        sha256: [i as u8; 32],
    });
    let manifest = Manifest {
        device_id: [1; 32],
        authority_id: [2; 32],
        boot_epoch: 3,
        request_id: 4,
        approved_loader_sha256: [5; 32],
        component_region_length: 64,
        entry_address: addresses[0],
        components,
    };
    let expected = ExpectedManifest {
        device_id: [1; 32],
        authority_id: [2; 32],
        approved_loader_sha256: [5; 32],
        boot_epoch: 3,
        request_id: 4,
    };
    let limits = ManifestLimits {
        max_cose_length: MAX_COSE_LEN as u32,
        max_component_region_length: 64,
        components: core::array::from_fn(|i| ComponentLimit {
            kind: kinds[i],
            load_address: addresses[i],
            max_load_end: addresses[i] + 0x1000,
            max_size: 0x1000,
            entry_address: if i == 0 { addresses[0] } else { 0 },
        }),
    };
    (manifest, expected, limits)
}

#[test]
fn manifest_limits_admit_only_exact_order_ranges_and_entry() {
    let (manifest, expected, limits) = values();
    assert_eq!(validate_manifest(&manifest, &expected, &limits), Ok(()));
    let mut bad = manifest;
    bad.components[1].kind = ComponentKind::Cellos;
    assert_eq!(
        validate_manifest(&bad, &expected, &limits),
        Err(Error::WrongComponent)
    );
    let mut bad = manifest;
    bad.components[1].offset += 1;
    assert_eq!(
        validate_manifest(&bad, &expected, &limits),
        Err(Error::LimitExceeded)
    );
    let mut bad = manifest;
    bad.components[1].load_address = bad.components[0].load_address;
    let mut permissive = limits;
    permissive.components[1].load_address = bad.components[1].load_address;
    assert_eq!(
        validate_manifest(&bad, &expected, &permissive),
        Err(Error::RangeOverlap)
    );
    let mut bad = manifest;
    bad.components[0].length = 0x1001;
    assert_eq!(
        validate_manifest(&bad, &expected, &limits),
        Err(Error::LimitExceeded)
    );
    let mut bad = manifest;
    bad.entry_address += 1;
    assert_eq!(
        validate_manifest(&bad, &expected, &limits),
        Err(Error::WrongEntry)
    );
    let mut bad = manifest;
    bad.component_region_length += 1;
    assert_eq!(
        validate_manifest(&bad, &expected, &limits),
        Err(Error::LimitExceeded)
    );
}

fn staging() -> StagingLimits {
    StagingLimits {
        usable_dram: PhysicalRange {
            base: 0x7000_0000,
            end: 0x9000_0000,
        },
        staging: PhysicalRange {
            base: 0x7000_0000,
            end: 0x7000_1000,
        },
        max_transfer_blocks: 4,
        manifest_bound: MAX_COSE_LEN as u32,
    }
}

#[test]
fn staging_requires_containment_alignment_capacity_and_disjointness() {
    let (_, _, limits) = values();
    assert_eq!(validate_staging(&staging(), &[], &limits), Ok(()));
    let mut bad = staging();
    bad.usable_dram.end = bad.usable_dram.base;
    assert_eq!(
        validate_staging(&bad, &[], &limits),
        Err(Error::InvalidStaging)
    );
    let mut bad = staging();
    bad.staging.base += 1;
    assert_eq!(
        validate_staging(&bad, &[], &limits),
        Err(Error::InvalidStaging)
    );
    let mut bad = staging();
    bad.staging.end = bad.staging.base + 1024;
    assert_eq!(
        validate_staging(&bad, &[], &limits),
        Err(Error::InvalidStaging)
    );
    let mut bad = staging();
    bad.staging.base = 0x6fff_f000;
    bad.staging.end = 0x7000_0000;
    assert_eq!(
        validate_staging(&bad, &[], &limits),
        Err(Error::InvalidStaging)
    );
    let mut outside = limits;
    outside.components[3].load_address = 0x9000_0000;
    outside.components[3].max_load_end = 0x9000_1000;
    assert_eq!(
        validate_staging(&staging(), &[], &outside),
        Err(Error::InvalidStaging)
    );
    let forbidden = [PhysicalRange {
        base: 0x7000_0800,
        end: 0x7000_1800,
    }];
    assert_eq!(
        validate_staging(&staging(), &forbidden, &limits),
        Err(Error::RangeOverlap)
    );
    let forbidden = [PhysicalRange {
        base: 0x8000_0000,
        end: 0x8000_1000,
    }];
    assert_eq!(
        validate_staging(&staging(), &forbidden, &limits),
        Err(Error::RangeOverlap)
    );
    let mut bad = staging();
    bad.staging.base = 0x8000_0000;
    bad.staging.end = 0x8000_1000;
    assert_eq!(
        validate_staging(&bad, &[], &limits),
        Err(Error::RangeOverlap)
    );
}

#[test]
fn checked_ranges_reject_empty_and_overflowing_inputs() {
    assert_eq!(PhysicalRange::new(1, 0), Err(Error::ZeroLength));
    assert_eq!(PhysicalRange::new(u64::MAX, 2), Err(Error::Overflow));
    let (_, _, limits) = values();
    let mut bad = staging();
    bad.staging.end = bad.staging.base - 1;
    assert_eq!(
        validate_staging(&bad, &[], &limits),
        Err(Error::InvalidStaging)
    );
}
