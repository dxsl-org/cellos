//! Host tests of the bring-up sequence against the fake MMIO: forcewake, identity, GGTT
//! entries in GSM, GuC reset, RSA hand-off, firmware DMA and the typed failures.

use crate::bring_up::{bring_up, guc_firmware_path, BringUpError, Platform, PINNED_GUC};
use crate::fake::FakeRegs;
use crate::ggtt::{
    GSM_OFFSET, GUC_TLB_INV_DESC0, GUC_TLB_INV_DESC0_VALID, GUC_TLB_INV_DESC1, WOPCM_SIZE,
};
use crate::guc::{
    GUC_DMA_ADDR_0_LOW, GUC_DMA_COPY_SIZE, GUC_STATUS, GUC_STATUS_UKERNEL_READY, UOS_RSA_SCRATCH_0,
};
use crate::identity::GfxArch;
use crate::regs::{FORCEWAKE_ACK_BIT, FORCEWAKE_ACK_GT, GDRST, GRDOM_GUC};
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use intel_gpu_core::guc_fw::{FwError, Version};
use types::ViError;

/// Device address the fake DMA authorization hands out.
const FW_IOVA: u64 = 0x4000_0000;

/// Firmware files and DMA authorization, recorded.
#[derive(Default)]
struct FakePlatform {
    file: Option<Vec<u8>>,
    loaded: Option<Vec<u8>>,
    requested: Option<String>,
    mapped: bool,
}

impl Platform for FakePlatform {
    fn read_firmware(&mut self, path: &str) -> Result<Option<usize>, ViError> {
        self.requested = Some(String::from(path));
        self.loaded = self.file.clone();
        Ok(self.loaded.as_ref().map(Vec::len))
    }

    fn with_firmware<T>(&mut self, f: impl FnOnce(&[u8]) -> T) -> T {
        f(self.loaded.as_deref().unwrap_or(&[]))
    }

    fn map_firmware(&mut self) -> Result<u64, ViError> {
        self.mapped = true;
        Ok(FW_IOVA)
    }
}

/// A CSS GuC image: header, `ucode_dw` uCode dwords and an RSA key of `key_dw` dwords
/// (key byte `i` = `i as u8`).
fn guc_blob(release: Version, ucode_dw: u32, key_dw: u32) -> Vec<u8> {
    let header_dw = 32 + key_dw;
    let mut blob = vec![0u8; 128 + (ucode_dw + key_dw) as usize * 4];
    let mut put = |off: usize, v: u32| blob[off..off + 4].copy_from_slice(&v.to_le_bytes());
    put(4, header_dw);
    put(24, header_dw + ucode_dw);
    put(28, key_dw);
    let sw = (u32::from(release.major) << 16)
        | (u32::from(release.minor) << 8)
        | u32::from(release.patch);
    put(64, sw);
    let rsa_at = 128 + ucode_dw as usize * 4;
    for (i, byte) in blob[rsa_at..].iter_mut().enumerate() {
        *byte = i as u8;
    }
    blob
}

fn platform_with(blob: Vec<u8>) -> FakePlatform {
    FakePlatform {
        file: Some(blob),
        ..FakePlatform::default()
    }
}

#[test]
fn bring_up_maps_firmware_in_ggtt_and_boots_the_guc() {
    let mut platform = platform_with(guc_blob(PINNED_GUC, 1000, 96)); // 384-byte RSA
    let ready = bring_up(FakeRegs::for_panther_lake(), &mut platform).expect("ready");
    let regs = &ready.controller.regs;

    assert_eq!(
        platform.requested.as_deref(),
        Some(guc_firmware_path(GfxArch::Xe3))
    );
    assert!(platform.mapped);
    // First GGTT address above WOPCM; one present WB entry per firmware page in GSM.
    assert_eq!(ready.guc_ggtt, WOPCM_SIZE);
    let len: usize = 128 + (1000 + 96) * 4;
    let first = GSM_OFFSET + (WOPCM_SIZE / 4096) as usize * 8;
    for page in 0..len.div_ceil(4096) {
        let pte =
            u64::from(regs.get(first + page * 8)) | u64::from(regs.get(first + page * 8 + 4)) << 32;
        assert_eq!(
            pte,
            (FW_IOVA + page as u64 * 4096) | 1 | 1 << 53,
            "page {page}"
        );
    }
    assert_eq!(regs.get(first + len.div_ceil(4096) * 8), 0);
    assert_eq!(regs.get(GUC_TLB_INV_DESC0), GUC_TLB_INV_DESC0_VALID);
    assert_ne!(regs.get(GUC_TLB_INV_DESC1), 0);

    // GuC reset requested, a >256-byte key handed over by GGTT address, and the DMA
    // copying the CSS header plus uCode (not the key) from the GGTT mapping.
    assert!(regs.writes.borrow().contains(&(GDRST, GRDOM_GUC)));
    assert_eq!(regs.get(UOS_RSA_SCRATCH_0) as u64, WOPCM_SIZE + 128 + 4000);
    assert_eq!(regs.get(GUC_DMA_ADDR_0_LOW) as u64, WOPCM_SIZE);
    assert_eq!(regs.get(GUC_DMA_COPY_SIZE), 128 + 4000);
    assert_eq!(regs.get(GUC_STATUS), GUC_STATUS_UKERNEL_READY);
    assert!(ready.controller.forcewake_active);
}

#[test]
fn small_rsa_key_fills_the_scratch_registers() {
    let mut platform = platform_with(guc_blob(PINNED_GUC, 16, 64)); // 256-byte RSA
    let ready = bring_up(FakeRegs::for_lunar_lake(), &mut platform).expect("ready");
    assert_eq!(
        platform.requested.as_deref(),
        Some(guc_firmware_path(GfxArch::Xe2))
    );
    let regs = &ready.controller.regs;
    assert_eq!(
        regs.get(UOS_RSA_SCRATCH_0),
        u32::from_le_bytes([0, 1, 2, 3])
    );
    assert_eq!(
        regs.get(UOS_RSA_SCRATCH_0 + 63 * 4),
        u32::from_le_bytes([252, 253, 254, 255])
    );
}

#[test]
fn missing_firmware_is_a_typed_error_and_releases_forcewake() {
    let regs = FakeRegs::for_panther_lake();
    let mut platform = FakePlatform::default();
    let err = bring_up(regs, &mut platform)
        .err()
        .expect("no firmware, not ready");
    assert_eq!(
        err,
        BringUpError::FirmwareUnavailable("/lib/firmware/xe/ptl_guc_70.bin")
    );
    assert!(!platform.mapped);
}

#[test]
fn unpinned_or_corrupt_firmware_never_reaches_the_gpu() {
    let other = Version::new(70, 53, 0);
    let mut platform = platform_with(guc_blob(other, 16, 96));
    assert_eq!(
        bring_up(FakeRegs::for_panther_lake(), &mut platform).err(),
        Some(BringUpError::FirmwareRejected(FwError::NotPinned {
            found: other,
            pinned: PINNED_GUC
        }))
    );
    assert!(!platform.mapped);

    let mut truncated = guc_blob(PINNED_GUC, 16, 96);
    truncated.truncate(200);
    let mut platform = platform_with(truncated);
    assert_eq!(
        bring_up(FakeRegs::for_panther_lake(), &mut platform).err(),
        Some(BringUpError::FirmwareRejected(FwError::Truncated))
    );
    assert!(!platform.mapped);
}

#[test]
fn guc_boot_failure_is_reported_not_ready() {
    let mut regs = FakeRegs::for_panther_lake();
    regs.guc_status_after_dma = 0x70 << 8; // XE_GUC_LOAD_STATUS_EXCEPTION
    let mut platform = platform_with(guc_blob(PINNED_GUC, 16, 96));
    assert_eq!(
        bring_up(regs, &mut platform).err(),
        Some(BringUpError::GucLoad(ViError::IO))
    );
}

#[test]
fn forcewake_failure_stops_before_firmware() {
    let mut regs = FakeRegs::for_panther_lake();
    regs.auto_ack_forcewake = false;
    let mut platform = platform_with(guc_blob(PINNED_GUC, 16, 96));
    assert_eq!(
        bring_up(regs, &mut platform).err(),
        Some(BringUpError::Probe(ViError::IO))
    );
    assert!(platform.requested.is_none());
}

#[test]
fn failed_bring_up_releases_forcewake() {
    // Observe the registers through a GuC failure: forcewake must be dropped again.
    let mut regs = FakeRegs::for_panther_lake();
    regs.guc_status_after_dma = 0x70 << 8;
    let ack = core::cell::RefCell::new(None);
    struct Probe<'a>(FakeRegs, &'a core::cell::RefCell<Option<u32>>);
    impl crate::regs::RegisterFile for Probe<'_> {
        fn read32(&self, offset: usize) -> types::ViResult<u32> {
            self.0.read32(offset)
        }
        fn write32(&mut self, offset: usize, value: u32) -> types::ViResult<()> {
            self.0.write32(offset, value)?;
            *self.1.borrow_mut() = Some(self.0.get(FORCEWAKE_ACK_GT));
            Ok(())
        }
    }
    let mut platform = platform_with(guc_blob(PINNED_GUC, 16, 96));
    assert!(bring_up(Probe(regs, &ack), &mut platform).is_err());
    assert_eq!(ack.borrow().unwrap() & FORCEWAKE_ACK_BIT, 0);
}
