//! GPU bring-up: forcewake -> GT identity -> GuC firmware -> GGTT -> GuC load -> ready.
//!
//! Upstream reference: the `xe_device_probe` / `xe_uc_load_hw` order in
//! `drivers/gpu/drm/xe/{xe_device,xe_uc,xe_guc}.c` (MIT). Every failure is a typed
//! [`BringUpError`]; nothing short of a running GuC is reported ready.

use crate::controller::IntelGpuController;
use crate::ggtt::Ggtt;
use crate::guc::load_guc;
use crate::identity::GfxArch;
use crate::regs::RegisterFile;
use intel_gpu_core::guc_fw::{self, FwError, Version};
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
    /// The GuC did not reset, take the DMA, or reach its running state.
    GucLoad(ViError),
}

/// A GPU whose GuC is running.
pub struct Ready<R: RegisterFile> {
    pub controller: IntelGpuController<R>,
    /// GGTT address of the GuC image.
    pub guc_ggtt: u64,
}

/// Bring the GPU behind `regs` up. On failure after forcewake, forcewake is released again.
pub fn bring_up<R: RegisterFile>(
    regs: R,
    platform: &mut impl Platform,
) -> Result<Ready<R>, BringUpError> {
    let mut controller = IntelGpuController::probe(regs).map_err(BringUpError::Probe)?;
    match load_firmware(&mut controller, platform) {
        Ok(guc_ggtt) => Ok(Ready {
            controller,
            guc_ggtt,
        }),
        Err(error) => {
            let _ = controller.shutdown();
            Err(error)
        }
    }
}

fn load_firmware<R: RegisterFile>(
    controller: &mut IntelGpuController<R>,
    platform: &mut impl Platform,
) -> Result<u64, BringUpError> {
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
    let guc_ggtt = Ggtt::new()
        .map(&mut controller.regs, iova, len, PatIndex::WB)
        .map_err(BringUpError::FirmwareMap)?;
    let rsa = image.rsa_offset..image.rsa_offset + image.rsa_len;
    platform
        .with_firmware(|blob| load_guc(&mut controller.regs, &image, &blob[rsa], guc_ggtt))
        .map_err(BringUpError::GucLoad)?;
    Ok(guc_ggtt)
}
