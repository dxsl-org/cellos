//! Payloads of `DRM_IOCTL_XE_DEVICE_QUERY` for Mesa `anv`.
//!
//! Upstream reference: `drivers/gpu/drm/xe/xe_query.c` and `xe_drm.h` (MIT). Every layout
//! below is the uapi struct byte for byte. The values come from a [`DeviceInfo`], which the
//! driver Cell fills from the probed GT registers; the device has one GT and system memory
//! only.

use crate::uapi::ioctl::EINVAL;
use alloc::vec;
use alloc::vec::Vec;

pub const DRM_XE_DEVICE_QUERY_ENGINES: u32 = 0;
pub const DRM_XE_DEVICE_QUERY_MEM_REGIONS: u32 = 1;
pub const DRM_XE_DEVICE_QUERY_CONFIG: u32 = 2;
pub const DRM_XE_DEVICE_QUERY_GT_LIST: u32 = 3;
pub const DRM_XE_DEVICE_QUERY_HWCONFIG: u32 = 4;
pub const DRM_XE_DEVICE_QUERY_GT_TOPOLOGY: u32 = 5;

pub const DRM_XE_TOPO_DSS_GEOMETRY: u16 = 1;
pub const DRM_XE_TOPO_DSS_COMPUTE: u16 = 2;
pub const DRM_XE_TOPO_SIMD16_EU_PER_DSS: u16 = 5;

/// `sizeof(struct drm_xe_engine)`.
const ENGINE_SIZE: usize = 32;
/// `sizeof(struct drm_xe_mem_region)`.
const MEM_REGION_SIZE: usize = 88;
/// `sizeof(struct drm_xe_gt)`.
const GT_SIZE: usize = 96;
/// `DRM_XE_QUERY_CONFIG_MAX_EXEC_QUEUE_PRIORITY + 1`.
const CONFIG_PARAMS: usize = 5;
/// `sizeof(xe_dss_mask_t)`: `XE_MAX_DSS_FUSE_BITS` (128) rounded to `unsigned long`s.
const DSS_MASK_BYTES: usize = 16;
/// `sizeof(xe_eu_mask_t)`: `XE_MAX_EU_FUSE_BITS` (32) rounded to one `unsigned long`.
const EU_MASK_BYTES: usize = 8;

/// What `DEVICE_QUERY` and `DRM_IOCTL_VERSION` report about the GPU.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceInfo {
    /// `drm_version.desc`.
    pub name: &'static str,
    /// GMD_ID graphics architecture (`drm_xe_gt.ip_ver_major`).
    pub ip_ver_major: u16,
    /// GMD_ID graphics release (`drm_xe_gt.ip_ver_minor`).
    pub ip_ver_minor: u16,
    /// GMD_ID revision (`drm_xe_gt.ip_ver_rev`).
    pub ip_ver_rev: u16,
    /// Command streamer timestamp frequency (`drm_xe_gt.reference_clock`).
    pub reference_clock_hz: u32,
    /// Geometry DSS fuse mask (`DRM_XE_TOPO_DSS_GEOMETRY`).
    pub dss_geometry: u128,
    /// Compute DSS fuse mask (`DRM_XE_TOPO_DSS_COMPUTE`).
    pub dss_compute: u128,
    /// SIMD16 EUs enabled in each DSS (`DRM_XE_TOPO_SIMD16_EU_PER_DSS`).
    pub eu_per_dss: u32,
    /// `drm_xe_mem_region.total_size` of the system-memory region: the bytes this
    /// device can make GPU-visible for clients.
    pub sysmem_bytes: u64,
}

/// The bytes `query_id` copies to `drm_xe_device_query.data`, or `EINVAL` for an unknown id.
pub fn query_payload(device: &DeviceInfo, query_id: u32) -> Result<Vec<u8>, i32> {
    match query_id {
        DRM_XE_DEVICE_QUERY_ENGINES => {
            // One RENDER engine, instance 0 on GT 0: all-zero `drm_xe_engine`.
            let mut out = vec![0u8; 8 + ENGINE_SIZE];
            put(&mut out, 0, &1u32.to_le_bytes());
            Ok(out)
        }
        DRM_XE_DEVICE_QUERY_MEM_REGIONS => {
            let mut out = vec![0u8; 8 + MEM_REGION_SIZE];
            put(&mut out, 0, &1u32.to_le_bytes());
            // mem_class SYSMEM (0), instance 0
            put(&mut out, 8 + 4, &4096u32.to_le_bytes()); // min_page_size
            put(&mut out, 8 + 8, &device.sysmem_bytes.to_le_bytes()); // total_size
            Ok(out)
        }
        DRM_XE_DEVICE_QUERY_CONFIG => {
            let mut out = vec![0u8; 8 + CONFIG_PARAMS * 8];
            put(&mut out, 0, &(CONFIG_PARAMS as u32).to_le_bytes());
            // info[0] REV_AND_DEVICE_ID and info[1] FLAGS stay 0.
            put(&mut out, 8 + 2 * 8, &4096u64.to_le_bytes()); // MIN_ALIGNMENT
            put(&mut out, 8 + 3 * 8, &48u64.to_le_bytes()); // VA_BITS
            put(&mut out, 8 + 4 * 8, &2u64.to_le_bytes()); // MAX_EXEC_QUEUE_PRIORITY
            Ok(out)
        }
        DRM_XE_DEVICE_QUERY_GT_LIST => {
            let mut out = vec![0u8; 8 + GT_SIZE];
            put(&mut out, 0, &1u32.to_le_bytes());
            let gt = 8;
            // type MAIN (0), tile 0, gt_id 0
            put(&mut out, gt + 12, &device.reference_clock_hz.to_le_bytes());
            put(&mut out, gt + 16, &1u64.to_le_bytes()); // near_mem_regions: sysmem
            put(&mut out, gt + 32, &device.ip_ver_major.to_le_bytes());
            put(&mut out, gt + 34, &device.ip_ver_minor.to_le_bytes());
            put(&mut out, gt + 36, &device.ip_ver_rev.to_le_bytes());
            Ok(out)
        }
        DRM_XE_DEVICE_QUERY_GT_TOPOLOGY => {
            // Entries are packed: each mask follows its 8-byte header directly.
            let mut out = Vec::with_capacity(3 * 8 + 2 * DSS_MASK_BYTES + EU_MASK_BYTES);
            let eu = u64::from(device.eu_per_dss).to_le_bytes();
            for (kind, mask) in [
                (
                    DRM_XE_TOPO_DSS_GEOMETRY,
                    &device.dss_geometry.to_le_bytes()[..],
                ),
                (
                    DRM_XE_TOPO_DSS_COMPUTE,
                    &device.dss_compute.to_le_bytes()[..],
                ),
                (DRM_XE_TOPO_SIMD16_EU_PER_DSS, &eu[..]),
            ] {
                out.extend_from_slice(&0u16.to_le_bytes()); // gt_id
                out.extend_from_slice(&kind.to_le_bytes());
                out.extend_from_slice(&(mask.len() as u32).to_le_bytes()); // num_bytes
                out.extend_from_slice(mask);
            }
            Ok(out)
        }
        _ => Err(EINVAL),
    }
}

fn put(out: &mut [u8], offset: usize, bytes: &[u8]) {
    out[offset..offset + bytes.len()].copy_from_slice(bytes);
}
