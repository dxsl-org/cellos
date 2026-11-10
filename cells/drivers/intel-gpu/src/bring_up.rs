//! GPU bring-up: forcewake -> GT identity -> GuC firmware -> GGTT -> WOPCM -> GuC reset ->
//! log, ADS and parameters -> transfer setup -> RSA -> uCode DMA -> boot -> CT enabled.
//!
//! Upstream reference: the `xe_device_probe` / `xe_uc_init` / `xe_uc_load_hw` order in
//! `drivers/gpu/drm/xe/{xe_device,xe_uc,xe_guc,xe_wopcm}.c` (MIT). Every failure is a typed
//! [`BringUpError`]; nothing short of a running GuC with an enabled CT is reported ready.

use crate::clock::Clock;
use crate::controller::IntelGpuController;
use crate::devmem::{DmaMemory, Pool, PAGE};
use crate::ggtt::Ggtt;
use crate::guc::{
    init_wopcm, prepare_xfer, reset_guc, transfer_rsa, upload_guc_firmware,
    wait_guc_ukernel_running, write_params, GucError, DIST_DBS_POPULATED, GUC_WOPCM_LOAD_OFFSET,
};
use crate::guc_ct::{CtFailure, GucCt};
use crate::identity::GfxArch;
use crate::regs::RegisterFile;
use intel_gpu_core::guc_ads;
use intel_gpu_core::guc_fw::{self, FwError, GucImage, Version, CSS_HEADER_LEN};
use intel_gpu_core::guc_init::{self, WopcmLayout, GUC_LOG_SIZE};
use intel_gpu_core::pte::PatIndex;
use types::ViError;

/// The GuC release ADR-0033 §5 pins (`research/firmware-pin.md`) for PTL/WCL and LNL.
pub const PINNED_GUC: Version = Version::new(70, 72, 1);

/// The linux-firmware GuC image for `arch`.
pub const fn guc_firmware_path(arch: GfxArch) -> &'static str {
    match arch {
        GfxArch::Xe2 => "/lib/firmware/xe/lnl_guc_70.bin",
        GfxArch::Xe3 => "/lib/firmware/xe/ptl_guc_70.bin",
    }
}

/// What bring-up needs from the system besides MMIO: the firmware file in memory the
/// GPU can read.
pub trait Platform {
    /// Read the file at `path`; its length, or `Ok(None)` when the image does not carry it.
    fn read_firmware(&mut self, path: &str) -> Result<Option<usize>, ViError>;
    /// Run `f` over the bytes [`Self::read_firmware`] loaded.
    fn with_firmware<T>(&mut self, f: impl FnOnce(&[u8]) -> T) -> T;
    /// Authorize the GPU to read the loaded firmware; returns its page-aligned device address.
    fn map_firmware(&mut self) -> Result<u64, ViError>;
}

/// The PCI identity the GuC parameter block carries (`guc_ctl_devid`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PciIdentity {
    pub device_id: u16,
    /// PCI revision. The kernel's `PcieDeviceInfo` record does not carry it, so the Cell
    /// passes 0 until it does.
    pub revision: u8,
}

/// Why the GPU did not come up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BringUpError {
    /// Forcewake was not acknowledged, or the GT identity is unreadable or unsupported.
    Probe(ViError),
    /// The image carries no GuC firmware at this path.
    FirmwareUnavailable(&'static str),
    /// The firmware file could not be read (I/O, or larger than the driver accepts).
    FirmwareRead(ViError),
    /// The file is not a valid GuC image, or not the pinned release.
    FirmwareRejected(FwError),
    /// The firmware could not be made device-readable or mapped into the GGTT.
    FirmwareMap(ViError),
    /// No device memory for the GuC log, ADS or CT buffers.
    NoMemory,
    /// The GuC did not partition, reset, take the DMA, or reach its running state.
    GucLoad(GucError),
    /// The GuC runs but its command transport could not be enabled.
    GucCt(CtFailure),
}

/// A GPU whose GuC is running with its CT channel enabled.
pub struct Ready<R: RegisterFile, C: Clock, M: DmaMemory> {
    pub controller: IntelGpuController<R, C>,
    pub mem: M,
    pub ggtt: Ggtt,
    pub pool: Pool,
    pub ct: GucCt,
    /// GGTT address of the GuC image.
    pub guc_ggtt: u64,
    pub wopcm: WopcmLayout,
    /// How long the GuC took to boot, in microseconds (upstream warns above 200 ms).
    pub load_us: u64,
}

/// Bring the GPU behind `regs` up. On failure after forcewake, forcewake is released again.
pub fn bring_up<R: RegisterFile, C: Clock, M: DmaMemory>(
    regs: R,
    clock: C,
    mut mem: M,
    pci: PciIdentity,
    platform: &mut impl Platform,
) -> Result<Ready<R, C, M>, BringUpError> {
    let mut controller = IntelGpuController::probe(regs, clock).map_err(BringUpError::Probe)?;
    let mut ggtt = Ggtt::new();
    let mut pool = Pool::default();
    match load_guc(
        &mut controller,
        &mut mem,
        &mut ggtt,
        &mut pool,
        pci,
        platform,
    ) {
        Ok(loaded) => Ok(Ready {
            controller,
            mem,
            ggtt,
            pool,
            ct: loaded.ct,
            guc_ggtt: loaded.guc_ggtt,
            wopcm: loaded.wopcm,
            load_us: loaded.load_us,
        }),
        Err(error) => {
            let _ = controller.shutdown();
            Err(error)
        }
    }
}

struct Loaded {
    ct: GucCt,
    guc_ggtt: u64,
    wopcm: WopcmLayout,
    load_us: u64,
}

fn load_guc<R: RegisterFile, C: Clock, M: DmaMemory>(
    controller: &mut IntelGpuController<R, C>,
    mem: &mut M,
    ggtt: &mut Ggtt,
    pool: &mut Pool,
    pci: PciIdentity,
    platform: &mut impl Platform,
) -> Result<Loaded, BringUpError> {
    let (image, guc_ggtt) = map_firmware(controller, ggtt, platform)?;
    let regs = &mut controller.regs;
    let clock = &mut controller.clock;
    let upload_size = CSS_HEADER_LEN + image.ucode_len;
    let gucerr = BringUpError::GucLoad;

    // `xe_uc_init` → `xe_wopcm_init`: the partition must hold header plus uCode.
    let guc_fw_size = u32::try_from(upload_size).map_err(|_| gucerr(GucError::BadImage))?;
    let wopcm = init_wopcm(regs, guc_fw_size).map_err(gucerr)?;
    // `xe_uc_sanitize_reset` → `xe_guc_reset`.
    reset_guc(regs, clock).map_err(gucerr)?;

    // `xe_guc_upload` → `xe_guc_ads_populate`; the log object (`xe_guc_log_init`).
    let log = pool
        .alloc(mem, regs, ggtt, (GUC_LOG_SIZE as usize).div_ceil(PAGE))
        .ok_or(BringUpError::NoMemory)?;
    let ads_size = guc_ads::ads_size(image.private_data_size);
    let ads = pool
        .alloc(mem, regs, ggtt, ads_size.div_ceil(PAGE))
        .ok_or(BringUpError::NoMemory)?;
    let doorbells = regs
        .read32(DIST_DBS_POPULATED)
        .map_err(|_| gucerr(GucError::Mmio))?;
    let blob = guc_ads::build(ads.ggtt, image.private_data_size, 1, doorbells);
    if !mem.write(ads.iova, &blob) {
        return Err(BringUpError::NoMemory);
    }

    // `__xe_guc_upload`: parameters, transfer setup, RSA, DMA at 8 KiB, boot wait.
    let params = guc_init::params(log.ggtt, ads.ggtt, pci.device_id, pci.revision);
    write_params(regs, &params).map_err(gucerr)?;
    prepare_xfer(regs).map_err(gucerr)?;
    let rsa = image.rsa_offset..image.rsa_offset + image.rsa_len;
    platform
        .with_firmware(|blob| transfer_rsa(regs, &blob[rsa], guc_ggtt + image.rsa_offset as u64))
        .map_err(gucerr)?;
    upload_guc_firmware(regs, clock, guc_ggtt, GUC_WOPCM_LOAD_OFFSET, upload_size)
        .map_err(gucerr)?;
    let load_us = wait_guc_ukernel_running(regs, clock).map_err(gucerr)?;

    // `xe_guc_enable_communication` → `xe_guc_ct_enable`. GuC-to-host interrupts stay
    // off: the driver polls the G2H ring.
    let ct = GucCt::enable(pool, mem, regs, ggtt, clock).map_err(BringUpError::GucCt)?;
    // The ADS and log regions are never freed: the GuC uses them for as long as it runs.
    Ok(Loaded {
        ct,
        guc_ggtt,
        wopcm,
        load_us,
    })
}

/// Read, validate and GGTT-map the pinned GuC image.
fn map_firmware<R: RegisterFile, C: Clock>(
    controller: &mut IntelGpuController<R, C>,
    ggtt: &mut Ggtt,
    platform: &mut impl Platform,
) -> Result<(GucImage, u64), BringUpError> {
    let path = guc_firmware_path(controller.identity.arch);
    let len = platform
        .read_firmware(path)
        .map_err(BringUpError::FirmwareRead)?
        .ok_or(BringUpError::FirmwareUnavailable(path))?;
    // Validate before the GPU may read a single byte of the file.
    let image = platform
        .with_firmware(|blob| {
            let image = guc_fw::parse(blob)?;
            guc_fw::check_pinned(&image, PINNED_GUC).map(|()| image)
        })
        .map_err(BringUpError::FirmwareRejected)?;
    let iova = platform.map_firmware().map_err(BringUpError::FirmwareMap)?;
    let guc_ggtt = ggtt
        .map(&mut controller.regs, iova, len, PatIndex::WB)
        .map_err(BringUpError::FirmwareMap)?;
    Ok((image, guc_ggtt))
}
