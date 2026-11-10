//! Host unit tests for probe, forcewake, identity parsing, GGTT and PPGTT building blocks.

use crate::batch::{build_store_marker_batch, simulate_gpu_execution};
use crate::controller::IntelGpuController;
use crate::fake::{FakeClock, FakeRegs};
use crate::forcewake::{forcewake_gt_acquire, forcewake_gt_release, FORCEWAKE_ACK_TIMEOUT_US};
use crate::guc::{upload_guc_firmware, GucError};
use crate::identity::{parse_gmd_id, read_ref_clock, read_topology, GfxArch};
use crate::ppgtt_store::{ClientVm, MemTableStore};
use crate::regs::{
    FORCEWAKE_ACK_BIT, FORCEWAKE_ACK_GT, FORCEWAKE_GT, GMD_ID, RPM_CONFIG0, XE2_GT_GEOMETRY_DSS_2,
};
use intel_gpu_core::pte::PatIndex;
use types::ViError;

#[test]
fn forcewake_handshake_success() {
    let mut regs = FakeRegs::new();
    let mut clock = FakeClock::default();
    assert_eq!(forcewake_gt_acquire(&mut regs, &mut clock), Ok(()));
    assert_eq!(regs.get(FORCEWAKE_GT), 0x0001_0001);
    assert_eq!(
        regs.get(FORCEWAKE_ACK_GT) & FORCEWAKE_ACK_BIT,
        FORCEWAKE_ACK_BIT
    );
    assert_eq!(forcewake_gt_release(&mut regs, &mut clock), Ok(()));
    assert_eq!(regs.get(FORCEWAKE_GT), 0x0001_0000);
    assert_eq!(regs.get(FORCEWAKE_ACK_GT) & FORCEWAKE_ACK_BIT, 0);
}

#[test]
fn forcewake_times_out_after_50_ms_when_unresponsive() {
    let mut regs = FakeRegs::new();
    regs.auto_ack_forcewake = false;
    let mut clock = FakeClock::default();
    assert_eq!(
        forcewake_gt_acquire(&mut regs, &mut clock),
        Err(ViError::IO)
    );
    assert!(clock.now >= FORCEWAKE_ACK_TIMEOUT_US && clock.now < FORCEWAKE_ACK_TIMEOUT_US + 10_000);
}

#[test]
fn forcewake_all_ones_ack_is_not_an_ack() {
    let mut regs = FakeRegs::new();
    regs.auto_ack_forcewake = false;
    regs.set(FORCEWAKE_ACK_GT, u32::MAX);
    assert_eq!(
        forcewake_gt_acquire(&mut regs, &mut FakeClock::default()),
        Err(ViError::IO)
    );
}

#[test]
fn gmd_id_decoding() {
    assert_eq!(parse_gmd_id((30 << 22) | 2), Ok((GfxArch::Xe3, 30, 0, 2)));
    assert_eq!(
        parse_gmd_id((20 << 22) | (4 << 14)),
        Ok((GfxArch::Xe2, 20, 4, 0))
    );
    assert_eq!(parse_gmd_id(12 << 22), Err(ViError::NotSupported));
}

#[test]
fn controller_probe_panther_lake_success() {
    let mut controller =
        IntelGpuController::probe(FakeRegs::for_panther_lake(), FakeClock::default()).unwrap();
    assert_eq!(controller.identity.arch, GfxArch::Xe3);
    assert_eq!(controller.identity.name(), "Intel Xe3 (PTL/WCL)");
    assert_eq!(controller.identity.dss_geometry_mask, 0x0FFF);
    assert_eq!(controller.identity.ref_clock_hz, 24_000_000);
    assert!(controller.forcewake_active);
    controller.shutdown().unwrap();
    assert!(!controller.forcewake_active);
    assert_eq!(controller.regs.get(FORCEWAKE_ACK_GT) & FORCEWAKE_ACK_BIT, 0);
}

#[test]
fn controller_probe_fails_on_unsupported_hardware() {
    let regs = FakeRegs::for_panther_lake();
    regs.set(GMD_ID, 9 << 22);
    assert!(IntelGpuController::probe(regs, FakeClock::default()).is_err());
}

#[test]
fn ref_clock_and_dss_mask_decode_upstream_fields() {
    let regs = FakeRegs::for_lunar_lake();
    regs.set(RPM_CONFIG0, 3 << 3);
    assert_eq!(read_ref_clock(&regs), Ok(25_000_000));
    regs.set(RPM_CONFIG0, 4 << 3);
    assert_eq!(read_ref_clock(&regs), Err(ViError::NotSupported));
    regs.set(XE2_GT_GEOMETRY_DSS_2, 1);
    let topology = read_topology(&regs).unwrap();
    assert_eq!(topology.dss_geometry, (1u128 << 64) | 0xFF);
    assert_eq!(topology.dss_compute, 0x3F);
}

#[test]
fn ggtt_pte_encoding_and_allocator() {
    use crate::ggtt::{encode_ggtt_pte, GgttAllocator};
    // WB is PAT index 2: only GGTT PAT1 (bit 53); bit 1 is device-memory, never set here.
    assert_eq!(
        encode_ggtt_pte(0x1000, PatIndex::WB),
        Ok(0x1000 | 1 | (1 << 53))
    );
    assert_eq!(
        encode_ggtt_pte(0x1001, PatIndex::WB),
        Err(ViError::InvalidArgument)
    );
    assert_eq!(
        encode_ggtt_pte(0x1000, PatIndex::WT),
        Err(ViError::InvalidArgument)
    );
    let mut alloc = GgttAllocator::new(0x10000, 0x10000);
    assert_eq!(alloc.allocate(4000), Ok(0x10000));
    assert_eq!(alloc.allocate(8192), Ok(0x11000));
    assert_eq!(alloc.allocate(0x100000), Err(ViError::OutOfMemory));
}

#[test]
fn guc_upload_invalid_size_rejected() {
    let mut regs = FakeRegs::for_panther_lake();
    let mut clock = FakeClock::default();
    for size in [0, 3] {
        assert_eq!(
            upload_guc_firmware(&mut regs, &mut clock, 0x1000, 0, size),
            Err(GucError::BadImage)
        );
    }
}

#[test]
fn client_vm_bind_translate_unbind() {
    let mut store = MemTableStore::default();
    let mut vm = ClientVm::new(&mut store).unwrap();
    vm.bind_range(&mut store, 0x10_0000, 0x20_0000, 0x4000, true, PatIndex::WB)
        .unwrap();
    for i in 0..4 {
        assert_eq!(
            vm.translate(&mut store, 0x10_0000 + i * 4096 + 0x123),
            Ok(Some(0x20_0000 + i * 4096 + 0x123))
        );
    }
    assert_eq!(
        vm.bind_range(&mut store, 0x10_0000, 0x20_0000, 4096, true, PatIndex::WB),
        Err(ViError::AlreadyExists)
    );
    vm.unbind_range(&mut store, 0x10_1000, 0x2000);
    assert_eq!(vm.translate(&mut store, 0x10_1000), Ok(None));
    assert_eq!(vm.translate(&mut store, 0x10_3000), Ok(Some(0x20_3000)));
    for (va, pa, len) in [
        (0x1001, 0x2000, 4096),
        (0x1000, 0x2001, 4096),
        (0x1000, 0x2000, 4095),
    ] {
        assert_eq!(
            vm.bind_range(&mut store, va, pa, len, true, PatIndex::WB),
            Err(ViError::InvalidArgument)
        );
    }
    vm.destroy(&mut store);
    assert!(store.tables.is_empty());
}

#[test]
fn refused_ppgtt_bind_rolls_back_its_pages() {
    let mut store = MemTableStore::default();
    let mut vm = ClientVm::new(&mut store).unwrap();
    vm.bind_range(&mut store, 0x3000, 0x9000, 4096, false, PatIndex::WB)
        .unwrap();
    assert_eq!(
        vm.bind_range(&mut store, 0x1000, 0x20000, 3 * 4096, true, PatIndex::WB),
        Err(ViError::AlreadyExists)
    );
    assert_eq!(vm.translate(&mut store, 0x1000), Ok(None));
    assert_eq!(vm.translate(&mut store, 0x2000), Ok(None));
    assert_eq!(vm.translate(&mut store, 0x3000), Ok(Some(0x9000)));
}

#[test]
fn first_light_marker_write_and_readback() {
    let mut store = MemTableStore::default();
    let mut vm = ClientVm::new(&mut store).unwrap();
    let base: u64 = 0x20_0000;
    let mut memory = alloc::vec![0u8; 64 * 1024];
    vm.bind_range(&mut store, 0x10_0000, base, 4096, true, PatIndex::WB)
        .unwrap();
    vm.bind_range(
        &mut store,
        0x11_0000,
        base + 0x4000,
        4096,
        true,
        PatIndex::WB,
    )
    .unwrap();
    for (i, word) in build_store_marker_batch(0x11_0000, 0xDEAD_BEEF)
        .iter()
        .enumerate()
    {
        memory[i * 4..i * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    let executed = simulate_gpu_execution(&vm, &mut store, 0x10_0000, &mut memory, base).unwrap();
    assert_eq!(executed, 2);
    assert_eq!(&memory[0x4000..0x4004], &0xDEAD_BEEFu32.to_le_bytes());
}
