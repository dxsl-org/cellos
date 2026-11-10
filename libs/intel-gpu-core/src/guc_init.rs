//! GuC boot parameters (`GUC_CTL_*`) and the WOPCM partition, computed before the
//! firmware transfer.
//!
//! Upstream reference (MIT): `drivers/gpu/drm/xe/xe_guc.c` (`guc_ctl_log_params_flags`,
//! `guc_ctl_feature_flags`, `guc_ctl_debug_flags`, `guc_ctl_ads_flags`, `guc_ctl_devid`,
//! `guc_init_params_post_hwconfig`, `guc_write_params`), `xe_guc_fwif.h` (`GUC_CTL_*`
//! bit layouts), `xe_guc_log.h` (buffer sizes of a non-debug build) and `xe_wopcm.c`
//! (`xe_wopcm_init`, `__check_layout`, `__wopcm_regs_locked`, `__wopcm_init_regs`).

/// `GUC_CTL_MAX_DWORDS`: parameters written to `SOFT_SCRATCH(1..=14)`.
pub const GUC_CTL_MAX_DWORDS: usize = 14;
pub const GUC_CTL_LOG_PARAMS: usize = 0;
pub const GUC_CTL_WA: usize = 1;
pub const GUC_CTL_FEATURE: usize = 2;
pub const GUC_CTL_DEBUG: usize = 3;
pub const GUC_CTL_ADS: usize = 4;
pub const GUC_CTL_DEVID: usize = 5;

pub const GUC_LOG_VALID: u32 = 1 << 0;
pub const GUC_LOG_NOTIFY_ON_HALF_FULL: u32 = 1 << 1;
pub const GUC_LOG_CAPTURE_ALLOC_UNITS: u32 = 1 << 2;
pub const GUC_LOG_LOG_ALLOC_UNITS: u32 = 1 << 3;
/// `GUC_CTL_ENABLE_LITE_RESTORE`.
pub const GUC_CTL_ENABLE_LITE_RESTORE: u32 = 1 << 4;
/// `GUC_CTL_ENABLE_SLPC`.
pub const GUC_CTL_ENABLE_SLPC: u32 = 1 << 2;
/// `GUC_LOG_DISABLED` in `GUC_CTL_DEBUG`.
pub const GUC_LOG_DISABLED: u32 = 1 << 6;

/// `XE_GUC_LOG_CRASH_DUMP_BUFFER_SIZE` (non-debug build).
pub const LOG_CRASH_DUMP_SIZE: u32 = 16 * 1024;
/// `XE_GUC_LOG_EVENT_DATA_BUFFER_SIZE` (non-debug build).
pub const LOG_EVENT_DATA_SIZE: u32 = 64 * 1024;
/// `XE_GUC_LOG_STATE_CAPTURE_BUFFER_SIZE` (non-debug build).
pub const LOG_STATE_CAPTURE_SIZE: u32 = 1024 * 1024;
/// `GUC_LOG_SIZE`: a 4 KiB header page plus the three buffers.
pub const GUC_LOG_SIZE: u32 =
    4096 + LOG_EVENT_DATA_SIZE + LOG_CRASH_DUMP_SIZE + LOG_STATE_CAPTURE_SIZE;

/// `guc_ctl_log_params_flags` for the non-debug sizes: the crash and event buffers are
/// not 1 MiB multiples, so they count in 4 KiB units; the capture buffer counts in 1 MiB
/// units (`GUC_LOG_CAPTURE_ALLOC_UNITS`).
pub const fn log_params(log_ggtt: u32) -> u32 {
    const LOG_UNIT: u32 = 4096;
    const CAPTURE_UNIT: u32 = 1024 * 1024;
    GUC_LOG_VALID
        | GUC_LOG_NOTIFY_ON_HALF_FULL
        | GUC_LOG_CAPTURE_ALLOC_UNITS
        | (LOG_CRASH_DUMP_SIZE / LOG_UNIT - 1) << 4
        | (LOG_EVENT_DATA_SIZE / LOG_UNIT - 1) << 6
        | (LOG_STATE_CAPTURE_SIZE / CAPTURE_UNIT - 1) << 10
        | (log_ggtt >> 12) << 12
}

/// `guc_ctl_feature_flags` for this driver: lite restore only. SLPC stays off because the
/// driver does not run GuC power management (`xe_guc_pc.c`), as upstream does on
/// `skip_guc_pc` platforms; `GUC_CTL_DISABLE_MULTI_QUEUE` is not needed from GuC 70.66 on
/// and the pinned release is 70.72.1.
pub const fn feature_flags() -> u32 {
    GUC_CTL_ENABLE_LITE_RESTORE
}

/// `guc_ctl_debug_flags` at `guc_log_level` 1 (non-verbose): verbose logging disabled.
pub const fn debug_flags() -> u32 {
    GUC_LOG_DISABLED
}

/// `guc_ctl_ads_flags`: the ADS page number in `GUC_ADS_ADDR` (bits 21:1).
pub const fn ads_flags(ads_ggtt: u32) -> u32 {
    (ads_ggtt >> 12) << 1
}

/// `guc_ctl_devid`.
pub const fn devid(device_id: u16, revision: u8) -> u32 {
    (device_id as u32) << 16 | revision as u32
}

/// The parameter block `guc_write_params` copies to `SOFT_SCRATCH(1 + i)`.
///
/// `GUC_CTL_WA` is 0: `guc_ctl_wa_flags` derives it from the `XE_GT_WA` rule tables
/// (`xe_wa_oob.rules`), which this driver does not carry. That is also the value upstream
/// boots its first (hwconfig) load with (`guc_init_params`).
pub fn params(
    log_ggtt: u32,
    ads_ggtt: u32,
    device_id: u16,
    revision: u8,
) -> [u32; GUC_CTL_MAX_DWORDS] {
    let mut p = [0u32; GUC_CTL_MAX_DWORDS];
    p[GUC_CTL_LOG_PARAMS] = log_params(log_ggtt);
    p[GUC_CTL_FEATURE] = feature_flags();
    p[GUC_CTL_DEBUG] = debug_flags();
    p[GUC_CTL_ADS] = ads_flags(ads_ggtt);
    p[GUC_CTL_WA] = 0;
    p[GUC_CTL_DEVID] = devid(device_id, revision);
    p
}

/// `LNL_WOPCM_SIZE`: graphics 20 and later (`xe_wopcm_size`, `platform >= XE_LUNARLAKE`).
pub const LNL_WOPCM_SIZE: u32 = 8 << 20;
/// `max_wopcm_size` for every platform this driver serves.
pub const MAX_WOPCM_SIZE: u32 = 8 << 20;
/// `WOPCM_RESERVED_SIZE`: reserved above the HuC image.
pub const WOPCM_RESERVED_SIZE: u32 = 16 * 1024;
/// `GUC_WOPCM_RESERVED`: reserved at the start of the GuC region.
pub const GUC_WOPCM_RESERVED: u32 = 16 * 1024;
/// `GUC_WOPCM_STACK_RESERVED`.
pub const GUC_WOPCM_STACK_RESERVED: u32 = 8 * 1024;
/// `WOPCM_HW_CTX_RESERVED`: reserved at the top for hardware contexts.
pub const WOPCM_HW_CTX_RESERVED: u32 = 32 * 1024 + 4 * 1024;
/// `GUC_WOPCM_OFFSET_SHIFT`: the GuC base is 16 KiB aligned.
pub const GUC_WOPCM_OFFSET_ALIGNMENT: u32 = 1 << 14;
/// `GUC_WOPCM_SIZE_MASK` (bits 31:12) of `GUC_WOPCM_SIZE` (0xc050).
pub const GUC_WOPCM_SIZE_MASK: u32 = 0xffff_f000;
/// `GUC_WOPCM_SIZE_LOCKED`.
pub const GUC_WOPCM_SIZE_LOCKED: u32 = 1 << 0;
/// `GUC_WOPCM_OFFSET_MASK` (bits 31:14) of `DMA_GUC_WOPCM_OFFSET` (0xc340).
pub const GUC_WOPCM_OFFSET_MASK: u32 = 0xffff_c000;
/// `GUC_WOPCM_OFFSET_VALID`.
pub const GUC_WOPCM_OFFSET_VALID: u32 = 1 << 0;

/// The GuC's part of WOPCM.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WopcmLayout {
    pub base: u32,
    pub size: u32,
}

/// Why the WOPCM cannot hold the firmware (`-E2BIG` upstream).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WopcmError {
    /// The GuC region overlaps the hardware-context reservation.
    InvalidLayout,
    /// The GuC region cannot hold the image plus its reserved space and stack.
    NoSpaceForGuc,
    /// The region below the GuC base cannot hold the HuC image plus its reservation.
    NoSpaceForHuc,
}

/// `__check_layout`.
pub fn check_layout(
    wopcm_size: u32,
    layout: WopcmLayout,
    guc_fw_size: u32,
    huc_fw_size: u32,
) -> Result<(), WopcmError> {
    let size = wopcm_size - WOPCM_HW_CTX_RESERVED;
    if layout.base >= size || layout.size > size - layout.base {
        return Err(WopcmError::InvalidLayout);
    }
    if layout.size < guc_fw_size + GUC_WOPCM_RESERVED + GUC_WOPCM_STACK_RESERVED {
        return Err(WopcmError::NoSpaceForGuc);
    }
    if layout.base < huc_fw_size + WOPCM_RESERVED_SIZE {
        return Err(WopcmError::NoSpaceForHuc);
    }
    Ok(())
}

/// The partition `xe_wopcm_init` computes when the registers are not locked yet: the GuC
/// base right above the HuC image and its reservation, the rest (minus the hardware
/// context reservation) to the GuC.
pub fn compute_layout(
    wopcm_size: u32,
    guc_fw_size: u32,
    huc_fw_size: u32,
) -> Result<WopcmLayout, WopcmError> {
    let ctx_rsvd = WOPCM_HW_CTX_RESERVED;
    let base = (huc_fw_size + WOPCM_RESERVED_SIZE).next_multiple_of(GUC_WOPCM_OFFSET_ALIGNMENT);
    let base = base.min(wopcm_size - ctx_rsvd);
    let size = (wopcm_size - ctx_rsvd - base) & GUC_WOPCM_SIZE_MASK;
    let layout = WopcmLayout { base, size };
    check_layout(wopcm_size, layout, guc_fw_size, huc_fw_size)?;
    Ok(layout)
}

/// `__wopcm_regs_locked`: the partition firmware (BIOS/IFWI) or an earlier load already
/// locked, from the raw `DMA_GUC_WOPCM_OFFSET` and `GUC_WOPCM_SIZE` values.
pub fn locked_layout(reg_offset: u32, reg_size: u32) -> Option<WopcmLayout> {
    (reg_size & GUC_WOPCM_SIZE_LOCKED != 0 && reg_offset & GUC_WOPCM_OFFSET_VALID != 0).then_some(
        WopcmLayout {
            base: reg_offset & GUC_WOPCM_OFFSET_MASK,
            size: reg_size & GUC_WOPCM_SIZE_MASK,
        },
    )
}

#[cfg(test)]
#[path = "guc_init/tests.rs"]
mod tests;
