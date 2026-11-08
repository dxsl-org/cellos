//! Host unit tests for Intel iGPU driver probe, forcewake, and identity parsing.

use crate::controller::IntelGpuController;
use crate::fake::FakeRegs;
use crate::forcewake::{forcewake_gt_acquire, forcewake_gt_release};
use crate::identity::{parse_gmd_id, GfxArch};
use crate::regs::{FORCEWAKE_ACK_BIT, FORCEWAKE_ACK_GT, FORCEWAKE_GT, GMD_ID};
use types::ViError;

#[test]
fn forcewake_handshake_success() {
    let mut regs = FakeRegs::new();
    assert_eq!(forcewake_gt_acquire(&mut regs), Ok(()));

    // Check register state
    assert_eq!(regs.get(FORCEWAKE_GT), 0x0001_0001);
    assert_eq!(
        regs.get(FORCEWAKE_ACK_GT) & FORCEWAKE_ACK_BIT,
        FORCEWAKE_ACK_BIT
    );

    // Release forcewake
    assert_eq!(forcewake_gt_release(&mut regs), Ok(()));
    assert_eq!(regs.get(FORCEWAKE_GT), 0x0001_0000);
    assert_eq!(regs.get(FORCEWAKE_ACK_GT) & FORCEWAKE_ACK_BIT, 0);
}

#[test]
fn forcewake_timeout_when_unresponsive() {
    let mut regs = FakeRegs::new();
    regs.auto_ack_forcewake = false; // Never sets ACK bit

    assert_eq!(forcewake_gt_acquire(&mut regs), Err(ViError::IO));
}

#[test]
fn gmd_id_decoding_panther_lake() {
    let raw = (30 << 22) | (0 << 14) | (2 << 6);
    let (arch, major, minor, rev) = parse_gmd_id(raw).unwrap();
    assert_eq!(arch, GfxArch::Xe3);
    assert_eq!((major, minor, rev), (30, 0, 2));
}

#[test]
fn gmd_id_decoding_lunar_lake() {
    let raw = (20 << 22) | (4 << 14) | (0 << 6);
    let (arch, major, minor, rev) = parse_gmd_id(raw).unwrap();
    assert_eq!(arch, GfxArch::Xe2);
    assert_eq!((major, minor, rev), (20, 4, 0));
}

#[test]
fn gmd_id_unsupported_version_refused() {
    let raw = (12 << 22) | (0 << 14) | (0 << 6); // Xe-LP
    assert_eq!(parse_gmd_id(raw), Err(ViError::NotSupported));
}

#[test]
fn controller_probe_panther_lake_success() {
    let regs = FakeRegs::for_panther_lake();
    let mut controller = IntelGpuController::probe(regs).unwrap();

    assert_eq!(controller.identity.arch, GfxArch::Xe3);
    assert_eq!(controller.identity.name(), "Intel Xe3 (PTL/WCL)");
    assert_eq!(controller.identity.ip_ver_major, 30);
    assert_eq!(controller.identity.dss_geometry_mask, 0x0FFF);
    assert_eq!(controller.identity.eu_enable_mask, 0xFF);
    assert_eq!(controller.identity.ref_clock_hz, 24_000_000);
    assert!(controller.forcewake_active);

    // Shutdown releases forcewake
    controller.shutdown().unwrap();
    assert!(!controller.forcewake_active);
    assert_eq!(controller.regs.get(FORCEWAKE_ACK_GT) & FORCEWAKE_ACK_BIT, 0);
}

#[test]
fn controller_probe_fails_on_unsupported_hardware() {
    let regs = FakeRegs::for_panther_lake();
    // Overwrite with unsupported GMD_ID
    regs.set(GMD_ID, 9 << 22);

    assert_eq!(IntelGpuController::probe(regs).is_err(), true);
}
#[test]
fn ggtt_pte_encoding() {
    use crate::ggtt::encode_ggtt_pte;
    use intel_gpu_core::pte::PatIndex;

    // 4K aligned address with writeable and WB PAT
    let pte = encode_ggtt_pte(0x1000, true, PatIndex::WB).unwrap();
    assert_eq!(pte & 0xFFF, 0x013); // PRESENT (1) | RW (2) | PAT1 (16)
    assert_eq!(pte & !0xFFF, 0x1000);

    // Misaligned address rejected
    assert_eq!(
        encode_ggtt_pte(0x1001, true, PatIndex::WB),
        Err(ViError::InvalidArgument)
    );
}

#[test]
fn ggtt_allocator_bump_and_alignment() {
    use crate::ggtt::GgttAllocator;

    let mut alloc = GgttAllocator::new(0x10000, 0x10000); // 64 KiB space
    let a1 = alloc.allocate(4000).unwrap(); // rounds up to 4K
    assert_eq!(a1, 0x10000);

    let a2 = alloc.allocate(8192).unwrap();
    assert_eq!(a2, 0x11000);

    // Oversize allocation rejected
    assert_eq!(alloc.allocate(0x100000), Err(ViError::OutOfMemory));
}

#[test]
fn guc_bootloader_and_firmware_upload() {
    use crate::guc::bring_up_guc;

    let mut regs = FakeRegs::for_panther_lake();
    let fw_phys = 0x200000;
    let fw_size = 64 * 1024; // 64 KiB

    assert_eq!(bring_up_guc(&mut regs, fw_phys, fw_size), Ok(()));

    // Verify registers updated after DMA transfer
    assert_eq!(
        regs.get(crate::guc::GUC_STATUS),
        crate::guc::GUC_STATUS_UKERNEL_READY
    );
}

#[test]
fn guc_upload_invalid_size_rejected() {
    use crate::guc::upload_guc_firmware;

    let mut regs = FakeRegs::for_panther_lake();
    assert_eq!(
        upload_guc_firmware(&mut regs, 0x1000, 0, 0),
        Err(ViError::InvalidArgument)
    );
    assert_eq!(
        upload_guc_firmware(&mut regs, 0x1000, 0, 3),
        Err(ViError::InvalidArgument)
    );
}
