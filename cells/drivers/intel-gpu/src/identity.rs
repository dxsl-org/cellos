//! Intel GT identity and capability discovery from hardware registers.
//!
//! Upstream reference: `drivers/gpu/drm/xe/{xe_pci,xe_gt_topology,xe_gt_clock}.c` (MIT).

use crate::regs::{
    RegisterFile, GMD_ID, RPM_CONFIG0, XE2_GT_GEOMETRY_DSS_1, XE2_GT_GEOMETRY_DSS_2, XELP_EU_ENABLE,
};
pub const VENDOR_INTEL: u16 = 0x8086;
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
    pub dss_geometry_mask: u64,
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
}

/// Decode the raw 32-bit `GMD_ID` register.
pub fn parse_gmd_id(raw: u32) -> ViResult<(GfxArch, u32, u32, u32)> {
    let major = (raw >> 22) & 0x3F;
    let minor = (raw >> 14) & 0xFF;
    let rev = (raw >> 6) & 0x3F;

    let arch = match major {
        20 => GfxArch::Xe2,
        30 => GfxArch::Xe3,
        _ => return Err(ViError::NotSupported),
    };

    Ok((arch, major, minor, rev))
}

/// Read reference clock frequency in Hz from `RPM_CONFIG0`.
pub fn read_ref_clock(regs: &impl RegisterFile) -> ViResult<u64> {
    let rpm = regs.read32(RPM_CONFIG0)?;
    // CTC shift and base crystal frequency decoding
    let crystal_sel = (rpm >> 3) & 0x3;
    let base_hz = match crystal_sel {
        0 => 24_000_000, // 24 MHz
        1 => 19_200_000, // 19.2 MHz
        2 => 38_400_000, // 38.4 MHz
        _ => 24_000_000,
    };
    Ok(base_hz)
}

/// Read topology fuses (Dual Sub-slices and Execution Units).
pub fn read_topology(regs: &impl RegisterFile) -> ViResult<(u64, u32)> {
    let dss1 = regs.read32(XE2_GT_GEOMETRY_DSS_1)? as u64;
    let dss2 = regs.read32(XE2_GT_GEOMETRY_DSS_2)? as u64;
    let dss_mask = (dss2 << 32) | dss1;

    let eu_mask = regs.read32(XELP_EU_ENABLE)?;
    Ok((dss_mask, eu_mask))
}

/// Complete GT discovery probe.
pub fn probe_gt(regs: &impl RegisterFile) -> ViResult<GtIdentity> {
    let raw_gmd = regs.read32(GMD_ID)?;
    let (arch, major, minor, rev) = parse_gmd_id(raw_gmd)?;
    let (dss_geometry_mask, eu_enable_mask) = read_topology(regs)?;
    let ref_clock_hz = read_ref_clock(regs)?;

    Ok(GtIdentity {
        arch,
        ip_ver_major: major,
        ip_ver_minor: minor,
        ip_ver_rev: rev,
        dss_geometry_mask,
        eu_enable_mask,
        ref_clock_hz,
    })
}
