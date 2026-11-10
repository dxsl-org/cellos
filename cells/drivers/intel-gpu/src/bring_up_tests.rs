//! Host tests of the bring-up sequence against the fake MMIO and GuC: forcewake, identity,
//! GGTT entries in GSM, WOPCM partition and lock bits, GuC reset, parameter block, ADS in
//! device memory, transfer setup, RSA hand-off, firmware DMA, boot status, CT registration,
//! and the typed, time-bounded failures.

use crate::bring_up::{
    bring_up, guc_firmware_path, BringUpError, PciIdentity, Platform, Ready, PINNED_GUC,
};
use crate::fake::{FakeClock, FakeDma, FakeRegs};
use crate::ggtt::{
    GSM_OFFSET, GUC_TLB_INV_DESC0, GUC_TLB_INV_DESC0_VALID, GUC_TLB_INV_DESC1, WOPCM_SIZE,
};
use crate::guc::*;
use crate::guc_ct::CtFailure;
use crate::identity::GfxArch;
use crate::regs::{FORCEWAKE_ACK_BIT, FORCEWAKE_ACK_GT, GDRST, GRDOM_GUC, PMINTRMSK};
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use intel_gpu_core::guc_abi::*;
use intel_gpu_core::guc_ads;
use intel_gpu_core::guc_fw::{FwError, Version};
use intel_gpu_core::guc_init::{self, WopcmError, WopcmLayout};
use types::ViError;

/// Device address the fake DMA authorization hands out.
const FW_IOVA: u64 = 0x4000_0000;
const PCI: PciIdentity = PciIdentity {
    device_id: 0xB080,
    revision: 0,
};

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
/// (key byte `i` = `i as u8`), with a 32 KiB private data area.
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
    put(120, 0x8000); // private_data_size
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

fn bring(
    regs: FakeRegs,
    platform: &mut FakePlatform,
) -> Result<Ready<FakeRegs, FakeClock, FakeDma>, BringUpError> {
    let dma = regs.dma();
    bring_up(regs, FakeClock::default(), dma, PCI, platform)
}

fn position(regs: &FakeRegs, offset: usize) -> usize {
    regs.writes
        .borrow()
        .iter()
        .position(|&(o, _)| o == offset)
        .unwrap_or_else(|| panic!("{offset:#x} never written"))
}

#[test]
fn bring_up_maps_firmware_in_ggtt_and_boots_the_guc() {
    let mut platform = platform_with(guc_blob(PINNED_GUC, 1000, 96)); // 384-byte RSA
    let ready = bring(FakeRegs::for_panther_lake(), &mut platform).expect("ready");
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
    assert_eq!(regs.get(GUC_TLB_INV_DESC0), GUC_TLB_INV_DESC0_VALID);
    assert_ne!(regs.get(GUC_TLB_INV_DESC1), 0);

    // A >256-byte key handed over by GGTT address, the DMA copying the CSS header plus
    // uCode (not the key) from the GGTT mapping into WOPCM at 8 KiB.
    assert_eq!(regs.get(UOS_RSA_SCRATCH_0) as u64, WOPCM_SIZE + 128 + 4000);
    assert_eq!(regs.get(GUC_DMA_ADDR_0_LOW) as u64, WOPCM_SIZE);
    assert_eq!(regs.get(GUC_DMA_ADDR_0_HIGH), DMA_ADDRESS_SPACE_GGTT);
    assert_eq!(regs.get(GUC_DMA_ADDR_1_LOW), 0x2000);
    assert_eq!(regs.get(GUC_DMA_ADDR_1_HIGH), DMA_ADDRESS_SPACE_WOPCM);
    assert_eq!(regs.get(GUC_DMA_COPY_SIZE), 128 + 4000);
    assert_eq!(regs.get(GUC_STATUS), GUC_STATUS_UKERNEL_READY);
    assert!(ready.controller.forcewake_active);
}

#[test]
fn load_sequence_follows_xe_uc_order() {
    let mut platform = platform_with(guc_blob(PINNED_GUC, 16, 64));
    let ready = bring(FakeRegs::for_panther_lake(), &mut platform).expect("ready");
    let regs = &ready.controller.regs;
    // `xe_uc_init` (WOPCM) → `xe_uc_sanitize_reset` (GDRST) → `guc_write_params` →
    // `guc_prepare_xfer` → `guc_xfer_rsa` → `uc_fw_xfer` → CT registration.
    let order = [
        position(regs, GUC_WOPCM_SIZE),
        position(regs, DMA_GUC_WOPCM_OFFSET),
        position(regs, GDRST),
        position(regs, SOFT_SCRATCH_0),
        position(regs, SOFT_SCRATCH_0 + 4),
        position(regs, GUC_SHIM_CONTROL),
        position(regs, GT_PM_CONFIG),
        position(regs, PMINTRMSK),
        position(regs, UOS_RSA_SCRATCH_0),
        position(regs, GUC_DMA_ADDR_0_LOW),
        position(regs, GUC_DMA_CTRL),
        position(regs, VF_SW_FLAG_0),
    ];
    assert!(order.windows(2).all(|w| w[0] < w[1]), "{order:?}");
    assert_eq!(regs.written(GDRST), [GRDOM_GUC]);
    // `guc_prepare_xfer` values for graphics 20+: read caches, clock gating, MOCS UC 3.
    assert_eq!(regs.get(GUC_SHIM_CONTROL), 0x0300_8602);
    assert_eq!(regs.get(GT_PM_CONFIG), 1);
    assert_eq!(regs.get(PMINTRMSK), !(1 << 9));
    // `DMA_CTRL`: start with UOS_MOVE, then UOS_MOVE cleared.
    assert_eq!(regs.written(GUC_DMA_CTRL), [0x0011_0011, 0x0010_0000]);
}

#[test]
fn wopcm_partition_is_written_and_locked() {
    let mut platform = platform_with(guc_blob(PINNED_GUC, 16, 64));
    let ready = bring(FakeRegs::for_panther_lake(), &mut platform).expect("ready");
    let regs = &ready.controller.regs;
    assert_eq!(regs.written(GUC_WOPCM_SIZE), [0x7f_3000]);
    assert_eq!(regs.written(DMA_GUC_WOPCM_OFFSET), [0x4000]);
    assert_eq!(regs.get(GUC_WOPCM_SIZE), 0x7f_3001);
    assert_eq!(regs.get(DMA_GUC_WOPCM_OFFSET), 0x4001);
    assert_eq!(
        ready.wopcm,
        WopcmLayout {
            base: 0x4000,
            size: 0x7f_3000
        }
    );
}

#[test]
fn a_partition_firmware_locked_is_kept_and_checked() {
    let regs = FakeRegs::for_panther_lake();
    regs.set(GUC_WOPCM_SIZE, 0x40_0001);
    regs.set(DMA_GUC_WOPCM_OFFSET, 0x8001);
    let mut platform = platform_with(guc_blob(PINNED_GUC, 16, 64));
    let ready = bring(regs, &mut platform).expect("ready");
    assert!(ready.controller.regs.written(GUC_WOPCM_SIZE).is_empty());
    assert_eq!(
        ready.wopcm,
        WopcmLayout {
            base: 0x8000,
            size: 0x40_0000
        }
    );

    // A locked region too small for the image is refused before the GuC is touched.
    let regs = FakeRegs::for_panther_lake();
    regs.set(GUC_WOPCM_SIZE, 0x1001);
    regs.set(DMA_GUC_WOPCM_OFFSET, 0x8001);
    let mut platform = platform_with(guc_blob(PINNED_GUC, 16, 64));
    assert_eq!(
        bring(regs, &mut platform).err(),
        Some(BringUpError::GucLoad(GucError::Wopcm(
            WopcmError::NoSpaceForGuc
        )))
    );
}

#[test]
fn wopcm_registers_that_do_not_lock_are_refused() {
    let mut regs = FakeRegs::for_panther_lake();
    regs.wopcm_lock_broken = true;
    let mut platform = platform_with(guc_blob(PINNED_GUC, 16, 64));
    assert_eq!(
        bring(regs, &mut platform).err(),
        Some(BringUpError::GucLoad(GucError::WopcmNotLocked(0)))
    );
}

#[test]
fn parameter_block_and_ads_reach_the_guc() {
    let mut platform = platform_with(guc_blob(PINNED_GUC, 16, 64));
    let ready = bring(FakeRegs::for_lunar_lake(), &mut platform).expect("ready");
    let regs = &ready.controller.regs;
    assert_eq!(regs.written(SOFT_SCRATCH_0), [0]);
    let params: Vec<u32> = (1..=14).map(|i| regs.get(SOFT_SCRATCH_0 + 4 * i)).collect();
    let log_ggtt = params[0] & 0xffff_f000;
    let ads_ggtt = (params[4] >> 1) << 12;
    assert_eq!(params[0], guc_init::log_params(log_ggtt));
    assert_eq!(params[1], 0); // WA
    assert_eq!(params[2], 0x10); // lite restore
    assert_eq!(params[3], 0x40); // verbose log disabled
    assert_eq!(params[5], 0xB080_0000); // devid << 16 | revision
    assert!(params[6..].iter().all(|&p| p == 0));
    assert!(log_ggtt as u64 >= WOPCM_SIZE && ads_ggtt > log_ggtt);

    // The ADS the GuC will read at that GGTT address.
    let ads = regs.ggtt_to_iova(ads_ggtt as u64);
    let dev = regs.device.borrow();
    let at = |off: usize| dev.read_u32(ads + off as u64);
    assert_eq!(at(guc_ads::ADS_SCHEDULER_POLICIES), ads_ggtt + 4572);
    assert_eq!(at(guc_ads::ADS_GT_SYSTEM_INFO), ads_ggtt + 4668);
    assert_eq!(at(guc_ads::ADS_PRIVATE_DATA), ads_ggtt + 40960);
    assert_eq!(at(guc_ads::SYSTEM_INFO_OFFSET + 512), 1); // RCS0 enabled
    assert_eq!(at(guc_ads::SYSTEM_INFO_OFFSET + 576 + 8), 4); // DIST_DBS_POPULATED 3 + 1
                                                              // The private data area (32 KiB) lies inside the allocation.
    let mut tail = [0u8; 4];
    assert!(dev.read(ads + 40960 + 0x8000 - 4, &mut tail));
}

#[test]
fn ct_is_registered_with_self_config_and_enabled() {
    let mut platform = platform_with(guc_blob(PINNED_GUC, 16, 64));
    let ready = bring(FakeRegs::for_panther_lake(), &mut platform).expect("ready");
    let (h2g, g2h) = ready.ct.ggtt();
    let dev = ready.controller.regs.device.borrow();
    assert!(dev.ct_enabled);
    let cfg = &dev.self_cfg;
    assert_eq!(cfg[&KLV_H2G_CTB_DESCRIPTOR_ADDR], u64::from(h2g));
    assert_eq!(cfg[&KLV_H2G_CTB_ADDR], u64::from(h2g + 4096));
    assert_eq!(cfg[&KLV_H2G_CTB_SIZE], 16 * 1024);
    assert_eq!(cfg[&KLV_G2H_CTB_DESCRIPTOR_ADDR], u64::from(g2h + 2048));
    assert_eq!(cfg[&KLV_G2H_CTB_ADDR], u64::from(g2h + 4096));
    assert_eq!(cfg[&KLV_G2H_CTB_SIZE], 128 * 1024);
    // The last MMIO request was `HOST2GUC_CONTROL_CTB(ENABLE)`, answered with success.
    let regs = &ready.controller.regs;
    assert_eq!(regs.get(VF_SW_FLAG_0 + 4), 1);
    assert_eq!(regs.get(VF_SW_FLAG_0), 0xF000_0000);
}

#[test]
fn guc_boot_status_progression_is_polled_every_10_ms() {
    let mut regs = FakeRegs::for_panther_lake();
    // START, GDT done, DPC ready, then READY.
    regs.guc_status_after_dma = vec![0x01 << 8, 0x10 << 8, 0x50 << 8, GUC_STATUS_UKERNEL_READY];
    let mut platform = platform_with(guc_blob(PINNED_GUC, 16, 64));
    let ready = bring(regs, &mut platform).expect("ready");
    assert_eq!(ready.load_us, 3 * LOAD_POLL_US);
}

#[test]
fn guc_that_never_finishes_booting_times_out_after_3_s() {
    let mut regs = FakeRegs::for_panther_lake();
    regs.guc_status_after_dma = vec![0x01 << 8];
    let mut platform = platform_with(guc_blob(PINNED_GUC, 16, 64));
    let err = bring(regs, &mut platform).err();
    assert_eq!(
        err,
        Some(BringUpError::GucLoad(GucError::LoadTimeout(0x01 << 8)))
    );
}

#[test]
fn guc_load_timeout_is_measured_in_time_not_tries() {
    let regs = FakeRegs::for_panther_lake();
    regs.set(GUC_STATUS, 0x01 << 8);
    let mut clock = FakeClock::default();
    assert_eq!(
        wait_guc_ukernel_running(&regs, &mut clock),
        Err(GucError::LoadTimeout(0x01 << 8))
    );
    assert_eq!(clock.now, LOAD_TIMEOUT_US);
}

#[test]
fn stuck_guc_reset_fails_after_5_ms_and_releases_forcewake() {
    let mut regs = FakeRegs::for_panther_lake();
    regs.stuck_reset = true;
    let mut clock = FakeClock::default();
    assert_eq!(
        reset_guc(&mut regs, &mut clock),
        Err(GucError::ResetTimeout(GRDOM_GUC))
    );
    assert!(clock.now >= GDRST_TIMEOUT_US && clock.now < GDRST_TIMEOUT_US + 1_000);

    let mut regs = FakeRegs::for_panther_lake();
    regs.stuck_reset = true;
    let mut platform = platform_with(guc_blob(PINNED_GUC, 16, 64));
    let device = regs.device.clone();
    let err = bring(regs, &mut platform).err();
    assert_eq!(
        err,
        Some(BringUpError::GucLoad(GucError::ResetTimeout(GRDOM_GUC)))
    );
    // Nothing reached the GuC after the failed reset.
    assert!(device.borrow().self_cfg.is_empty());
}

#[test]
fn guc_boot_failure_is_reported_not_ready() {
    let mut regs = FakeRegs::for_panther_lake();
    regs.guc_status_after_dma = vec![0x70 << 8]; // XE_GUC_LOAD_STATUS_EXCEPTION
    let mut platform = platform_with(guc_blob(PINNED_GUC, 16, 96));
    assert_eq!(
        bring(regs, &mut platform).err(),
        Some(BringUpError::GucLoad(GucError::LoadFailed(0x70 << 8)))
    );
}

#[test]
fn load_status_decoding_follows_guc_load_done() {
    assert_eq!(guc_load_done(0xF0 << 8), LoadState::Running);
    for ukernel in [
        0x02, 0x03, 0x04, 0x07, 0x08, 0x60, 0x70, 0x71, 0x73, 0x74, 0x75, 0x76,
    ] {
        assert_eq!(
            guc_load_done(ukernel << 8),
            LoadState::Failed,
            "ukernel {ukernel:#x}"
        );
    }
    for bootrom in [0x13, 0x2B, 0x50, 0x73, 0x74, 0x75, 0x77, 0x79, 0x7A, 0x7E] {
        assert_eq!(
            guc_load_done(bootrom << 1),
            LoadState::Failed,
            "bootrom {bootrom:#x}"
        );
    }
    // Intermediate codes, including the boot ROM's JUMP_PASSED and hwconfig progress.
    for status in [
        0,
        0x01 << 8,
        0x05 << 8,
        0x06 << 8,
        0x50 << 8,
        0x76 << 1,
        0x1A << 1,
    ] {
        assert_eq!(
            guc_load_done(status),
            LoadState::InProgress,
            "status {status:#x}"
        );
    }
}

#[test]
fn dma_that_never_completes_times_out_after_100_ms() {
    let regs = FakeRegs::for_panther_lake();
    let mut clock = FakeClock::default();
    // START stays set: the engine never finishes.
    let start = u32::from(GUC_DMA_CTRL_START);
    regs.set(GUC_DMA_CTRL, start);
    let r = crate::regs::wait32(&regs, &mut clock, GUC_DMA_CTRL, start, 0, DMA_TIMEOUT_US);
    assert_eq!(r, Err(crate::regs::WaitError::TimedOut(start)));
    assert!(clock.now >= DMA_TIMEOUT_US);
}

#[test]
fn mmio_channel_failures_are_typed() {
    let mut regs = FakeRegs::for_panther_lake();
    regs.mmio_reply = Some(0xE012_0034);
    let mut platform = platform_with(guc_blob(PINNED_GUC, 16, 64));
    assert_eq!(
        bring(regs, &mut platform).err(),
        Some(BringUpError::GucCt(CtFailure::Setup(
            GucError::MmioFailure {
                hint: 0x12,
                error: 0x34
            }
        )))
    );
    // No origin bit ever: no reply within 50 ms.
    let mut regs = FakeRegs::for_panther_lake();
    regs.mmio_reply = Some(0x0508);
    let mut clock = FakeClock::default();
    assert_eq!(
        mmio_send(&mut regs, &mut clock, &self_cfg(KLV_H2G_CTB_SIZE, 1, 4096)),
        Err(GucError::MmioTimeout(0x0508))
    );
    assert!(clock.now >= MMIO_REPLY_TIMEOUT_US);
}

#[test]
fn small_rsa_key_fills_the_scratch_registers() {
    let mut platform = platform_with(guc_blob(PINNED_GUC, 16, 64)); // 256-byte RSA
    let ready = bring(FakeRegs::for_lunar_lake(), &mut platform).expect("ready");
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
    let mut platform = FakePlatform::default();
    let err = bring(FakeRegs::for_panther_lake(), &mut platform)
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
        bring(FakeRegs::for_panther_lake(), &mut platform).err(),
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
        bring(FakeRegs::for_panther_lake(), &mut platform).err(),
        Some(BringUpError::FirmwareRejected(FwError::Truncated))
    );
    assert!(!platform.mapped);
}

#[test]
fn forcewake_failure_stops_before_firmware() {
    let mut regs = FakeRegs::for_panther_lake();
    regs.auto_ack_forcewake = false;
    let mut platform = platform_with(guc_blob(PINNED_GUC, 16, 96));
    assert_eq!(
        bring(regs, &mut platform).err(),
        Some(BringUpError::Probe(ViError::IO))
    );
    assert!(platform.requested.is_none());
}

#[test]
fn failed_bring_up_releases_forcewake() {
    let mut regs = FakeRegs::for_panther_lake();
    regs.guc_status_after_dma = vec![0x70 << 8];
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
    let dma = regs.dma();
    let mut platform = platform_with(guc_blob(PINNED_GUC, 16, 96));
    assert!(bring_up(
        Probe(regs, &ack),
        FakeClock::default(),
        dma,
        PCI,
        &mut platform
    )
    .is_err());
    assert_eq!(ack.borrow().unwrap() & FORCEWAKE_ACK_BIT, 0);
}
