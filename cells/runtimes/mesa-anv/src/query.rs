//! Implementation of `DRM_IOCTL_XE_DEVICE_QUERY` for Mesa `anv`.
//!
//! Upstream reference: `drivers/gpu/drm/xe/xe_query.c` and `xe_drm.h` (MIT).

use crate::ioctl_defs::EINVAL;
use alloc::vec;
use alloc::vec::Vec;

pub const DRM_XE_DEVICE_QUERY_ENGINES: u32 = 0;
pub const DRM_XE_DEVICE_QUERY_MEM_REGIONS: u32 = 1;
pub const DRM_XE_DEVICE_QUERY_CONFIG: u32 = 2;
pub const DRM_XE_DEVICE_QUERY_GT_LIST: u32 = 3;
pub const DRM_XE_DEVICE_QUERY_HWCONFIG: u32 = 4;
pub const DRM_XE_DEVICE_QUERY_GT_TOPOLOGY: u32 = 5;

pub fn handle_device_query(
    query_id: u32,
    size_in_out: &mut u32,
    data_out: Option<&mut [u8]>,
) -> Result<(), i32> {
    let payload = generate_query_payload(query_id)?;
    let required_len = payload.len() as u32;

    if *size_in_out == 0 {
        // Size query step
        *size_in_out = required_len;
        return Ok(());
    }

    if *size_in_out < required_len {
        return Err(EINVAL);
    }

    if let Some(dest) = data_out {
        if dest.len() < payload.len() {
            return Err(EINVAL);
        }
        dest[..payload.len()].copy_from_slice(&payload);
    }

    *size_in_out = required_len;
    Ok(())
}

fn generate_query_payload(query_id: u32) -> Result<Vec<u8>, i32> {
    match query_id {
        DRM_XE_DEVICE_QUERY_ENGINES => {
            // struct drm_xe_query_engines: num_engines (u32), pad (u32), engines[]
            // struct drm_xe_engine_class_instance: engine_class (u16), engine_instance (u16), gt_id (u16), pad (u16)
            let mut out = vec![0u8; 8 + 8]; // 1 engine
            out[0..4].copy_from_slice(&1u32.to_le_bytes()); // num_engines = 1
                                                            // engine 0: RENDER (0), instance 0, gt_id 0
            out[8..10].copy_from_slice(&0u16.to_le_bytes()); // class = RENDER
            out[10..12].copy_from_slice(&0u16.to_le_bytes()); // instance = 0
            out[12..14].copy_from_slice(&0u16.to_le_bytes()); // gt_id = 0
            Ok(out)
        }
        DRM_XE_DEVICE_QUERY_MEM_REGIONS => {
            // struct drm_xe_query_mem_regions: num_mem_regions (u32), pad (u32), mem_regions[]
            // struct drm_xe_mem_region: mem_class (u16), instance (u16), min_page_size (u32), total_size (u64), used (u64), pad[]
            let mut out = vec![0u8; 8 + 40]; // 1 region
            out[0..4].copy_from_slice(&1u32.to_le_bytes()); // num_mem_regions = 1
            let reg_off = 8;
            out[reg_off..reg_off + 2].copy_from_slice(&0u16.to_le_bytes()); // SYSMEM = 0
            out[reg_off + 2..reg_off + 4].copy_from_slice(&0u16.to_le_bytes()); // instance = 0
            out[reg_off + 4..reg_off + 8].copy_from_slice(&4096u32.to_le_bytes()); // min_page_size = 4096
            out[reg_off + 8..reg_off + 16]
                .copy_from_slice(&(16u64 * 1024 * 1024 * 1024).to_le_bytes()); // total 16 GiB
            out[reg_off + 16..reg_off + 24].copy_from_slice(&0u64.to_le_bytes()); // used = 0
            Ok(out)
        }
        DRM_XE_DEVICE_QUERY_CONFIG => {
            // struct drm_xe_query_config: num_params (u32), pad (u32), info[16] (u64)
            let mut out = vec![0u8; 8 + 16 * 8];
            out[0..4].copy_from_slice(&16u32.to_le_bytes());
            // info[0] = FLAGS (0)
            // info[1] = MIN_ALIGNMENT (4096)
            out[8 + 8..8 + 16].copy_from_slice(&4096u64.to_le_bytes());
            // info[2] = VA_BITS (48)
            out[8 + 16..8 + 24].copy_from_slice(&48u64.to_le_bytes());
            // info[3] = MAX_EXEC_QUEUE_PRIORITY (2 = HIGH)
            out[8 + 24..8 + 32].copy_from_slice(&2u64.to_le_bytes());
            Ok(out)
        }
        DRM_XE_DEVICE_QUERY_GT_LIST => {
            // struct drm_xe_query_gt_list: num_gt (u32), pad (u32), gt_list[]
            // struct drm_xe_gt: type (u16), instance (u16), clock_freq (u32), ip_ver_major (u16), minor (u8), rev (u8)
            let mut out = vec![0u8; 8 + 64];
            out[0..4].copy_from_slice(&1u32.to_le_bytes()); // 1 GT
            let gt_off = 8;
            out[gt_off..gt_off + 2].copy_from_slice(&0u16.to_le_bytes()); // type = MAIN (0)
            out[gt_off + 2..gt_off + 4].copy_from_slice(&0u16.to_le_bytes()); // instance = 0
            out[gt_off + 4..gt_off + 8].copy_from_slice(&24_000_000u32.to_le_bytes()); // 24 MHz
            out[gt_off + 8..gt_off + 10].copy_from_slice(&30u16.to_le_bytes()); // Xe3 major = 30
            out[gt_off + 10] = 0; // minor = 0
            out[gt_off + 11] = 0; // rev = 0
            Ok(out)
        }
        DRM_XE_DEVICE_QUERY_GT_TOPOLOGY => {
            // struct drm_xe_query_topology_mask: gt_id (u16), type (u16), num_bytes (u32), mask[]
            let mut out = vec![0u8; 8 + 8];
            out[0..2].copy_from_slice(&0u16.to_le_bytes()); // gt_id = 0
            out[2..4].copy_from_slice(&0u16.to_le_bytes()); // type = DSS_GEOMETRY (0)
            out[4..8].copy_from_slice(&8u32.to_le_bytes()); // 8 bytes mask
            out[8..16].copy_from_slice(&0x0FFFu64.to_le_bytes()); // 12 DSS enabled
            Ok(out)
        }
        _ => Err(EINVAL),
    }
}
