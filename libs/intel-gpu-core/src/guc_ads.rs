//! The GuC Additional Data Structures (ADS) blob for a GT on which the driver submits only
//! to render engine instance 0.
//!
//! Upstream reference (MIT): `drivers/gpu/drm/xe/xe_guc_ads.c` (`struct __guc_ads_blob`,
//! the offset helpers, `guc_policies_init`, `fill_engine_enable_masks`,
//! `guc_golden_lrc_init`, `guc_mapping_table_init`, `guc_doorbell_init`,
//! `xe_guc_ads_populate_minimal`, `xe_guc_ads_populate`), `xe_guc_fwif.h` (`struct
//! guc_ads`, `guc_policies`, `guc_gt_system_info`, `guc_engine_usage`,
//! `guc_um_init_params`), `abi/guc_capture_abi.h` (`guc_mmio_reg_set`) and `xe_lrc.c`
//! (`xe_gt_lrc_size`, `xe_lrc_engine_state_size`).
//!
//! What the pinned GuC 70.x requires, and why each field is here:
//! - `scheduler_policies`, `gt_system_info` and `private_data` pointers, with the policies
//!   marked valid: `xe_guc_ads_populate_minimal` writes exactly these, and upstream boots the
//!   firmware with that minimal blob on every probe, so the firmware is known to accept
//!   everything else being zero;
//! - a private-data area of the CSS header's `private_data_size`: GuC scratch it owns;
//! - for submission, the engine-enable mask and the logical-to-physical mapping table of
//!   the render engine (`xe_guc_ads_populate`), else the GuC knows no engine to run a
//!   context on, and the golden-context address and engine-state size of the render
//!   class (`guc_golden_lrc_init`), which the GuC reads when it resets an engine;
//! - the doorbell count per SQIDI from `DIST_DBS_POPULATED` (`guc_doorbell_init`).
//!
//! Left zero, as in the minimal blob: the engine register save/restore lists
//! (`guc_mmio_reg_state_init`), the error-capture lists, the workaround KLVs and the
//! golden context contents (upstream records them after the first submission,
//! `xe_gt_record_default_lrcs`). Consequence: after a GuC engine reset the engine restarts
//! from zeroed state without workaround registers restored.

use crate::guc_abi::{GUC_MAX_ENGINE_CLASSES, GUC_MAX_INSTANCES_PER_CLASS, GUC_RENDER_CLASS};
use crate::lrc::{PPHWSP_SIZE, RENDER_IMAGE_SIZE};
use alloc::vec;
use alloc::vec::Vec;

const PAGE: usize = 4096;

/// `sizeof(struct guc_ads)`.
pub const GUC_ADS_SIZE: usize =
    16 * 32 * 8 + 5 * 4 + 2 * 16 * 4 + 2 * 4 + 2 * 2 * 16 * 4 + 2 * 4 + 3 * 4 + 11 * 4;
/// `offsetof(struct guc_ads, scheduler_policies)`.
pub const ADS_SCHEDULER_POLICIES: usize = 16 * 32 * 8 + 4;
/// `offsetof(struct guc_ads, gt_system_info)`.
pub const ADS_GT_SYSTEM_INFO: usize = ADS_SCHEDULER_POLICIES + 4;
/// `offsetof(struct guc_ads, golden_context_lrca)`.
pub const ADS_GOLDEN_CONTEXT_LRCA: usize = ADS_GT_SYSTEM_INFO + 3 * 4;
/// `offsetof(struct guc_ads, eng_state_size)`.
pub const ADS_ENG_STATE_SIZE: usize = ADS_GOLDEN_CONTEXT_LRCA + 16 * 4;
/// `offsetof(struct guc_ads, private_data)`.
pub const ADS_PRIVATE_DATA: usize = ADS_ENG_STATE_SIZE + 16 * 4;

/// `sizeof(struct guc_policies)`.
pub const POLICIES_SIZE: usize = 16 * 4 + 4 * 4 + 4 * 4;
/// `offsetof(struct __guc_ads_blob, policies)`.
pub const POLICIES_OFFSET: usize = GUC_ADS_SIZE;
/// `sizeof(struct guc_gt_system_info)`.
pub const SYSTEM_INFO_SIZE: usize = 16 * 32 + 16 * 4 + 16 * 4;
/// `offsetof(struct __guc_ads_blob, system_info)`.
pub const SYSTEM_INFO_OFFSET: usize = POLICIES_OFFSET + POLICIES_SIZE;
/// `sizeof(struct guc_engine_usage)`.
pub const ENGINE_USAGE_SIZE: usize = 16 * 32 * 32;
/// `sizeof(struct guc_um_init_params)`.
pub const UM_INIT_PARAMS_SIZE: usize = 8 + 6 * 4 + 3 * 32;
/// `guc_ads_regset_offset`: where the dynamic part starts.
pub const REGSET_OFFSET: usize =
    SYSTEM_INFO_OFFSET + SYSTEM_INFO_SIZE + ENGINE_USAGE_SIZE + UM_INIT_PARAMS_SIZE;

/// `xe_gt_lrc_size(gt, XE_ENGINE_CLASS_RENDER)` for graphics 20.xx-30.xx: PPHWSP plus the
/// engine image, no indirect ring state.
pub const RENDER_LRC_SIZE: usize = (PPHWSP_SIZE + RENDER_IMAGE_SIZE) as usize;
/// `xe_lrc_reg_size` for graphics 12.50 and later.
pub const LRC_REG_SIZE: usize = 96 * 4;
/// `xe_lrc_engine_state_size(gt, XE_ENGINE_CLASS_RENDER)`.
pub const RENDER_ENGINE_STATE_SIZE: usize = RENDER_IMAGE_SIZE as usize - LRC_REG_SIZE;

/// `guc_ads_golden_lrc_offset` with no register save/restore lists.
pub const GOLDEN_LRC_OFFSET: usize = REGSET_OFFSET.next_multiple_of(PAGE);
/// `guc_ads_private_data_offset`: after the golden render context; the workaround KLV and
/// capture areas are empty.
pub const PRIVATE_DATA_OFFSET: usize = (GOLDEN_LRC_OFFSET + RENDER_LRC_SIZE).next_multiple_of(PAGE);

/// `GLOBAL_POLICY_DEFAULT_DPC_PROMOTE_TIME_US`.
pub const DPC_PROMOTE_TIME_US: u32 = 500_000;
/// `GLOBAL_POLICY_MAX_NUM_WI`.
pub const MAX_NUM_WORK_ITEMS: u32 = 15;
/// `GUC_GENERIC_GT_SYSINFO_DOORBELL_COUNT_PER_SQIDI`.
pub const SYSINFO_DOORBELL_COUNT_PER_SQIDI: usize = 2;
/// `DOORBELLS_PER_SQIDI_MASK` (bits 23:16) of `DIST_DBS_POPULATED` (0xd08).
pub const DOORBELLS_PER_SQIDI_SHIFT: u32 = 16;

/// `guc_ads_size`: the blob plus the firmware's private data, page aligned.
pub const fn ads_size(private_data_size: u32) -> usize {
    PRIVATE_DATA_OFFSET + (private_data_size as usize).next_multiple_of(PAGE)
}

/// Build the ADS blob that will sit at GGTT address `ggtt`.
///
/// `render_instances` is the mask of render engine instances the GuC may schedule on
/// (bit 0 = RCS0); `dist_dbs_populated` is the raw `DIST_DBS_POPULATED` register.
pub fn build(
    ggtt: u32,
    private_data_size: u32,
    render_instances: u32,
    dist_dbs_populated: u32,
) -> Vec<u8> {
    let mut blob = vec![0u8; ads_size(private_data_size)];
    let mut put =
        |offset: usize, value: u32| blob[offset..offset + 4].copy_from_slice(&value.to_le_bytes());

    // `guc_policies_init`.
    put(POLICIES_OFFSET + 16 * 4, DPC_PROMOTE_TIME_US);
    put(POLICIES_OFFSET + 16 * 4 + 8, MAX_NUM_WORK_ITEMS);
    put(POLICIES_OFFSET + 16 * 4 + 12, 0); // global_flags
    put(POLICIES_OFFSET + 16 * 4 + 4, 1); // is_valid

    // `fill_engine_enable_masks`: only the render class is driven.
    let masks = SYSTEM_INFO_OFFSET + GUC_MAX_ENGINE_CLASSES * GUC_MAX_INSTANCES_PER_CLASS;
    put(masks + GUC_RENDER_CLASS as usize * 4, render_instances);

    // `guc_doorbell_init` (graphics 12+, integrated).
    let sysinfo = masks + GUC_MAX_ENGINE_CLASSES * 4;
    put(
        sysinfo + SYSINFO_DOORBELL_COUNT_PER_SQIDI * 4,
        (dist_dbs_populated >> DOORBELLS_PER_SQIDI_SHIFT & 0xff) + 1,
    );

    // `guc_golden_lrc_init` for the enabled class.
    if render_instances != 0 {
        put(
            ADS_ENG_STATE_SIZE + GUC_RENDER_CLASS as usize * 4,
            RENDER_ENGINE_STATE_SIZE as u32,
        );
        put(
            ADS_GOLDEN_CONTEXT_LRCA + GUC_RENDER_CLASS as usize * 4,
            ggtt + GOLDEN_LRC_OFFSET as u32,
        );
    }

    put(ADS_SCHEDULER_POLICIES, ggtt + POLICIES_OFFSET as u32);
    put(ADS_GT_SYSTEM_INFO, ggtt + SYSTEM_INFO_OFFSET as u32);
    put(ADS_PRIVATE_DATA, ggtt + PRIVATE_DATA_OFFSET as u32);

    // `guc_mapping_table_init`: every slot invalid (`GUC_MAX_INSTANCES_PER_CLASS`) except
    // the enabled render instances, which map to themselves.
    let table = &mut blob[SYSTEM_INFO_OFFSET..masks];
    table.fill(GUC_MAX_INSTANCES_PER_CLASS as u8);
    for instance in 0..GUC_MAX_INSTANCES_PER_CLASS {
        if render_instances >> instance & 1 != 0 {
            table[GUC_RENDER_CLASS as usize * GUC_MAX_INSTANCES_PER_CLASS + instance] =
                instance as u8;
        }
    }
    blob
}

#[cfg(test)]
#[path = "guc_ads/tests.rs"]
mod tests;
