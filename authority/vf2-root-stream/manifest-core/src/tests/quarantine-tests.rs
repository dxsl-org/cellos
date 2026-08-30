use crate::{
    CleanupHook, ComponentKind, ComponentLimit, Error, LogicalQuarantine, ManifestLimits,
    PhysicalRange, Result, StagingLimits,
};

struct RecordingCleanup {
    calls: usize,
    fail: bool,
}

impl CleanupHook for RecordingCleanup {
    fn make_visible(&mut self, bytes: &mut [u8]) -> Result<()> {
        self.calls += 1;
        if bytes.iter().any(|byte| *byte != 0) {
            return Err(Error::InvalidStaging);
        }
        if self.fail {
            return Err(Error::InvalidStaging);
        }
        Ok(())
    }
}

#[test]
fn invalid_limits_do_not_touch_storage_or_cleanup_hook() {
    let mut storage = [0xa5; 4096];
    let mut cleanup = RecordingCleanup {
        calls: 0,
        fail: false,
    };
    let mut staging = staging_limits();
    staging.staging.base = staging.usable_dram.end;
    staging.staging.end = staging.usable_dram.end + 4096;
    let error = match LogicalQuarantine::prepare(
        &mut storage,
        &mut cleanup,
        &staging,
        &[],
        &manifest_limits(),
    ) {
        Ok(quarantine) => {
            drop(quarantine);
            panic!("invalid limits were accepted")
        }
        Err(error) => error,
    };
    assert_eq!(error, Error::InvalidStaging);
    assert_eq!(storage, [0xa5; 4096]);
    assert_eq!(cleanup.calls, 0);
}

#[test]
fn prepare_without_incoming_manifest_then_finish_and_drop_in_order() {
    let mut storage = [0xa5; 4096];
    let mut cleanup = RecordingCleanup {
        calls: 0,
        fail: false,
    };
    {
        let mut quarantine = LogicalQuarantine::prepare(
            &mut storage,
            &mut cleanup,
            &staging_limits(),
            &[],
            &manifest_limits(),
        )
        .unwrap();
        assert!(quarantine.receive_buffer().iter().all(|byte| *byte == 0));
        quarantine.receive_buffer()[17] = 9;
        quarantine.finish().unwrap();
    }
    assert!(storage.iter().all(|byte| *byte == 0));
    assert_eq!(cleanup.calls, 2);

    {
        let mut quarantine = LogicalQuarantine::prepare(
            &mut storage,
            &mut cleanup,
            &staging_limits(),
            &[],
            &manifest_limits(),
        )
        .unwrap();
        quarantine.receive_buffer()[31] = 7;
    }
    assert!(storage.iter().all(|byte| *byte == 0));
    assert_eq!(cleanup.calls, 4);
}

#[test]
fn failed_visibility_hook_leaves_logical_storage_cleared() {
    let mut storage = [0xa5; 4096];
    let mut cleanup = RecordingCleanup {
        calls: 0,
        fail: true,
    };
    let error = match LogicalQuarantine::prepare(
        &mut storage,
        &mut cleanup,
        &staging_limits(),
        &[],
        &manifest_limits(),
    ) {
        Ok(quarantine) => {
            drop(quarantine);
            panic!("failed visibility hook was accepted")
        }
        Err(error) => error,
    };
    assert_eq!(error, Error::InvalidStaging);
    assert!(storage.iter().all(|byte| *byte == 0));
    assert_eq!(cleanup.calls, 1);
}

fn staging_limits() -> StagingLimits {
    StagingLimits {
        usable_dram: PhysicalRange {
            base: 0x8000_0000,
            end: 0x9000_0000,
        },
        staging: PhysicalRange {
            base: 0x8800_0000,
            end: 0x8800_1000,
        },
        max_transfer_blocks: 4,
        manifest_bound: 549,
    }
}

fn manifest_limits() -> ManifestLimits {
    let kinds = [
        ComponentKind::OpenSbi,
        ComponentKind::Dtb,
        ComponentKind::Cellos,
        ComponentKind::Vifs,
    ];
    ManifestLimits {
        max_cose_length: 549,
        max_component_region_length: 1024,
        components: core::array::from_fn(|index| {
            let load_address = 0x8020_0000 + index as u64 * 0x10_0000;
            ComponentLimit {
                kind: kinds[index],
                load_address,
                max_load_end: load_address + 0x1_0000,
                max_size: 0x1_0000,
                entry_address: if index == 0 { load_address } else { 0 },
            }
        }),
    }
}
