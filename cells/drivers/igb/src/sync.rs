//! Intel I350 hardware synchronization, EEE and DMA coalescing controls.
//!
//! Intel Ethernet Controller I350 Datasheet §4.3 (Power Management and EEE)
//! and §8.24 (Software / Firmware Synchronization).

use ostd::mmio::MmioRegion;
use types::{ViError, ViResult};

// ── Register Offsets ─────────────────────────────────────────────────────────

pub(crate) const EEER: usize = 0x0_0E30;
pub(crate) const EEE_SU: usize = 0x0_0E34;
pub(crate) const DMACR: usize = 0x0_2508;
pub(crate) const SWSM: usize = 0x0_5B50;
pub(crate) const SW_FW_SYNC: usize = 0x0_5B5C;

// ── Bits ─────────────────────────────────────────────────────────────────────

pub(crate) const SWSM_SMBI: u32 = 1 << 0;

pub(crate) const SWFW_EEP_SM: u32 = 0x0001;
pub(crate) const SWFW_PHY0_SM: u32 = 0x0002;
pub(crate) const SWFW_PHY1_SM: u32 = 0x0004;
pub(crate) const SWFW_PHY2_SM: u32 = 0x0020;
pub(crate) const SWFW_PHY3_SM: u32 = 0x0040;

const SEM_POLLS: u32 = 100_000;

/// Mask for port PHY synchronization in `SW_FW_SYNC`.
pub(crate) const fn phy_mask_for_port(port: u8) -> u32 {
    match port {
        0 => SWFW_PHY0_SM,
        1 => SWFW_PHY1_SM,
        2 => SWFW_PHY2_SM,
        _ => SWFW_PHY3_SM,
    }
}

/// Mask for EEPROM / NVM synchronization in `SW_FW_SYNC`.
pub(crate) const fn eep_mask() -> u32 {
    SWFW_EEP_SM
}

// ── Hardware Semaphore (SWSM) ────────────────────────────────────────────────

fn acquire_hw_semaphore(mmio: &MmioRegion) -> ViResult<()> {
    for _ in 0..SEM_POLLS {
        let swsm = mmio.read_u32(SWSM)?;
        if swsm & SWSM_SMBI == 0 {
            mmio.write_u32(SWSM, swsm | SWSM_SMBI)?;
            if mmio.read_u32(SWSM)? & SWSM_SMBI != 0 {
                return Ok(());
            }
        }
        core::hint::spin_loop();
    }
    Err(ViError::WouldBlock)
}

fn release_hw_semaphore(mmio: &MmioRegion) -> ViResult<()> {
    let swsm = mmio.read_u32(SWSM)? & !SWSM_SMBI;
    mmio.write_u32(SWSM, swsm)
}

// ── SW / FW Synchronization (SW_FW_SYNC) ─────────────────────────────────────

/// Acquire software-firmware synchronization for the resources indicated by `mask`.
pub(crate) fn acquire_swfw_sync(mmio: &MmioRegion, mask: u32) -> ViResult<()> {
    let fw_mask = mask << 16;
    for _ in 0..SEM_POLLS {
        acquire_hw_semaphore(mmio)?;
        let swfw = mmio.read_u32(SW_FW_SYNC)?;
        if swfw & (mask | fw_mask) == 0 {
            mmio.write_u32(SW_FW_SYNC, swfw | mask)?;
            let _ = release_hw_semaphore(mmio);
            return Ok(());
        }
        let _ = release_hw_semaphore(mmio);
        core::hint::spin_loop();
    }
    Err(ViError::WouldBlock)
}

/// Release software-firmware synchronization for the resources indicated by `mask`.
pub(crate) fn release_swfw_sync(mmio: &MmioRegion, mask: u32) -> ViResult<()> {
    acquire_hw_semaphore(mmio)?;
    let swfw = mmio.read_u32(SW_FW_SYNC)? & !mask;
    let res = mmio.write_u32(SW_FW_SYNC, swfw);
    let _ = release_hw_semaphore(mmio);
    res
}

/// Run `op` under `SW_FW_SYNC`, guaranteeing release on every exit path.
pub(crate) fn with_swfw_sync<R>(
    mmio: &MmioRegion,
    mask: u32,
    op: impl FnOnce() -> ViResult<R>,
) -> ViResult<R> {
    acquire_swfw_sync(mmio, mask)?;
    let result = op();
    let release_res = release_swfw_sync(mmio, mask);
    match (result, release_res) {
        (Ok(val), Ok(())) => Ok(val),
        (Err(e), _) => Err(e),
        (Ok(_), Err(e)) => Err(e),
    }
}

// ── I350 Feature Initialization ──────────────────────────────────────────────

/// Disable Energy Efficient Ethernet (EEE) and DMA coalescing for I350.
pub(crate) fn init_i350_features(mmio: &MmioRegion) -> ViResult<()> {
    mmio.write_u32(EEER, 0)?;
    mmio.write_u32(EEE_SU, 0)?;
    mmio.write_u32(DMACR, 0)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phy_mask_maps_all_four_ports() {
        assert_eq!(phy_mask_for_port(0), SWFW_PHY0_SM);
        assert_eq!(phy_mask_for_port(1), SWFW_PHY1_SM);
        assert_eq!(phy_mask_for_port(2), SWFW_PHY2_SM);
        assert_eq!(phy_mask_for_port(3), SWFW_PHY3_SM);
        assert_eq!(eep_mask(), SWFW_EEP_SM);
    }
}
