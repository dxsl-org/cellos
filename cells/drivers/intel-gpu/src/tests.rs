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
