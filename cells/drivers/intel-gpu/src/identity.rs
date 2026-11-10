//! Intel GT identity and capability discovery from hardware registers.
//!
//! Upstream reference: `drivers/gpu/drm/xe/{xe_pci,xe_gt_topology,xe_gt_clock}.c` (MIT).

use crate::regs::{
    RegisterFile, GMD_ID, RPM_CONFIG0, XE2_GT_COMPUTE_DSS_2, XE2_GT_GEOMETRY_DSS_1,
    XE2_GT_GEOMETRY_DSS_2, XEHPC_GT_COMPUTE_DSS_ENABLE_EXT, XEHP_GT_COMPUTE_DSS_ENABLE,
    XELP_EU_ENABLE, XELP_GT_GEOMETRY_DSS_ENABLE,
};
pub const VENDOR_INTEL: u16 = 0x8086;
use intel_gpu_core::uapi::query::DeviceInfo;
use types::{ViError, ViResult};

/// Recognized Intel graphics generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GfxArch {
    /// Xe2 generation (Lunar Lake, Gfx version 20.04).
    Xe2,
    /// Xe3 generation (Panther Lake / Wildcat Lake, Gfx version 30.00).
    Xe3,
}

/// Discovered hardware GT identity and topology.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GtIdentity {
    pub arch: GfxArch,
    pub ip_ver_major: u32,
    pub ip_ver_minor: u32,
    pub ip_ver_rev: u32,
    /// Geometry DSS enable bits across the three Xe2/Xe3 fuse registers (96 bits).
    pub dss_geometry_mask: u128,
    /// Compute DSS enable bits across the three Xe2/Xe3 fuse registers (96 bits).
    pub dss_compute_mask: u128,
    /// SIMD16 EUs per DSS: on Xe2 and later one fuse bit is one EU (`load_eu_mask`).
    pub eu_enable_mask: u32,
    pub ref_clock_hz: u64,
}

impl GtIdentity {
    pub const fn name(&self) -> &'static str {
        match self.arch {
            GfxArch::Xe2 => "Intel Xe2 (LNL)",
            GfxArch::Xe3 => "Intel Xe3 (PTL/WCL)",
        }
    }

    /// What `DEVICE_QUERY` reports for this GT. `sysmem_bytes` is the system memory the
    /// driver can make GPU-visible for clients.
    pub fn device_info(&self, sysmem_bytes: u64) -> DeviceInfo {
        DeviceInfo {
            name: self.name(),
            // GMD_ID fields are 10/8/6 bits wide, so these never truncate.
            ip_ver_major: self.ip_ver_major as u16,
            ip_ver_minor: self.ip_ver_minor as u16,
            ip_ver_rev: self.ip_ver_rev as u16,
            // `read_ref_clock` yields one of four crystal rates, all below 2^32.
            reference_clock_hz: self.ref_clock_hz as u32,
            dss_geometry: self.dss_geometry_mask,
            dss_compute: self.dss_compute_mask,
            eu_per_dss: self.eu_enable_mask,
            sysmem_bytes,
        }
    }
}

/// Decode the raw 32-bit `GMD_ID` register: arch 31:22, release 21:14, revid 5:0.
pub fn parse_gmd_id(raw: u32) -> ViResult<(GfxArch, u32, u32, u32)> {
    let major = raw >> 22;
    let minor = (raw >> 14) & 0xFF;
    let rev = raw & 0x3F;

    let arch = match major {
        20 => GfxArch::Xe2,
        30 => GfxArch::Xe3,
        _ => return Err(ViError::NotSupported),
    };

    Ok((arch, major, minor, rev))
}

/// Read the crystal reference clock in Hz from `RPM_CONFIG0` bits 5:3 (`xe_gt_clock.c`).
pub fn read_ref_clock(regs: &impl RegisterFile) -> ViResult<u64> {
    let rpm = regs.read32(RPM_CONFIG0)?;
    match (rpm >> 3) & 0x7 {
        0 => Ok(24_000_000),
        1 => Ok(19_200_000),
        2 => Ok(38_400_000),
        3 => Ok(25_000_000),
        _ => Err(ViError::NotSupported),
    }
}

/// Fuse topology of one GT (`xe_gt_topology.c`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Topology {
    pub dss_geometry: u128,
    pub dss_compute: u128,
    pub eu_per_dss: u32,
}

/// Read the geometry and compute DSS masks and the EU enable mask.
pub fn read_topology(regs: &impl RegisterFile) -> ViResult<Topology> {
    let geometry = [
        XELP_GT_GEOMETRY_DSS_ENABLE,
        XE2_GT_GEOMETRY_DSS_1,
        XE2_GT_GEOMETRY_DSS_2,
    ];
    let compute = [
        XEHP_GT_COMPUTE_DSS_ENABLE,
        XEHPC_GT_COMPUTE_DSS_ENABLE_EXT,
        XE2_GT_COMPUTE_DSS_2,
    ];
    Ok(Topology {
        dss_geometry: read_mask(regs, &geometry)?,
        dss_compute: read_mask(regs, &compute)?,
        eu_per_dss: regs.read32(XELP_EU_ENABLE)?,
    })
}

/// Concatenate fuse registers, the first one holding bits 31:0.
fn read_mask(regs: &impl RegisterFile, words: &[usize; 3]) -> ViResult<u128> {
    let mut mask = 0u128;
    for (word, &reg) in words.iter().enumerate() {
        mask |= u128::from(regs.read32(reg)?) << (32 * word);
    }
    Ok(mask)
}

/// Complete GT discovery probe.
pub fn probe_gt(regs: &impl RegisterFile) -> ViResult<GtIdentity> {
    let raw_gmd = regs.read32(GMD_ID)?;
    let (arch, major, minor, rev) = parse_gmd_id(raw_gmd)?;
    let topology = read_topology(regs)?;
    let ref_clock_hz = read_ref_clock(regs)?;

    Ok(GtIdentity {
        arch,
        ip_ver_major: major,
        ip_ver_minor: minor,
        ip_ver_rev: rev,
        dss_geometry_mask: topology.dss_geometry,
        dss_compute_mask: topology.dss_compute,
        eu_enable_mask: topology.eu_per_dss,
        ref_clock_hz,
    })
}
