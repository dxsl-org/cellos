//! RMRR (Reserved Memory Region Reporting) support (VT-d spec §8.4).

use super::context::canonical_bdf;
use crate::acpi::{DeviceScope, RmrrEntry};
use crate::dev::iommu_pt::VtdSlpt;
use alloc::vec::Vec;

const SCOPE_PCI_ENDPOINT: u8 = 1;
const SCOPE_PCI_SUB_HIERARCHY: u8 = 2;
const CFG_SECONDARY_BUS: usize = 0x19;
const CFG_SUBORDINATE_BUS: usize = 0x1A;

/// Bus-number register `off` of the bridge at `bus:dev.func`, from config space.
fn bridge_bus(bus: u8, dev: u8, func: u8, off: usize) -> Option<u8> {
    let bdf = canonical_bdf(bus, dev, func) as u32;
    let ptr = crate::dev::pcie_ecam::config_ptr(bdf, off)?;
    // SAFETY: `config_ptr` returns an address inside the identity-mapped ECAM window.
    Some(unsafe { core::ptr::read_volatile(ptr) })
}

/// Function a scope path ends at: every hop but the last is a bridge whose
/// secondary bus carries the next hop. `None` for an empty or truncated path.
pub(super) fn scope_target(scope: &DeviceScope) -> Option<(u8, u8, u8)> {
    if scope.path_len == 0 || scope.path_truncated() {
        return None;
    }
    let (&[dev, func], hops) = scope.path[..scope.path_len as usize].split_last()?;
    let mut bus = scope.start_bus;
    for &[hop_dev, hop_func] in hops {
        bus = bridge_bus(bus, hop_dev, hop_func, CFG_SECONDARY_BUS)?;
    }
    Some((bus, dev, func))
}

pub fn device_matches_scope(bdf: u16, scope: &DeviceScope) -> bool {
    let bus = ((bdf >> 8) & 0xFF) as u8;
    let dev = ((bdf >> 3) & 0x1F) as u8;
    let func = (bdf & 0x07) as u8;

    let Some(target) = scope_target(scope) else {
        return false;
    };
    let is_target = target == (bus, dev, func);
    match scope.kind {
        SCOPE_PCI_ENDPOINT => is_target,
        // The bridge itself and every bus behind it.
        SCOPE_PCI_SUB_HIERARCHY if is_target => true,
        SCOPE_PCI_SUB_HIERARCHY => {
            let (t_bus, t_dev, t_func) = target;
            let secondary = bridge_bus(t_bus, t_dev, t_func, CFG_SECONDARY_BUS);
            let subordinate = bridge_bus(t_bus, t_dev, t_func, CFG_SUBORDINATE_BUS);
            match (secondary, subordinate) {
                (Some(first), Some(last)) => first > t_bus && (first..=last).contains(&bus),
                _ => false,
            }
        }
        _ => false,
    }
}

pub fn matches_rmrr(bdf: u16, segment: u16, rmrr: &RmrrEntry) -> bool {
    if rmrr.segment != segment {
        return false;
    }
    for scope in rmrr.scopes.as_slice() {
        if device_matches_scope(bdf, scope) {
            return true;
        }
    }
    false
}

pub fn rmrrs_for_bdf(bdf: u16, segment: u16) -> Vec<(u64, u64)> {
    let mut ranges = Vec::new();
    crate::acpi::with_topology(|topo| {
        for rmrr in topo.rmrrs.as_slice() {
            if matches_rmrr(bdf, segment, rmrr) {
                ranges.push((rmrr.base, rmrr.limit));
            }
        }
    });
    ranges
}

pub fn apply_rmrr_to_slpt(slpt: &VtdSlpt, bdf: u16, segment: u16) {
    let ranges = rmrrs_for_bdf(bdf, segment);
    for (base, limit) in ranges {
        if limit >= base {
            let size = ((limit - base + 1) as usize).next_multiple_of(4096);
            slpt.map_range(base, size);
            log::info!(
                "[vtd] rmrr kept for bdf {:04x}: [{:#x}, {:#x}]",
                bdf,
                base,
                limit
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_scope_matching() {
        let mut scope = DeviceScope::ZERO;
        scope.kind = 1;
        scope.start_bus = 0;
        scope.path_len = 1;
        scope.path[0] = [2, 0]; // dev 2 func 0 (e.g. iGPU 00:02.0)

        let igpu_bdf = (2 << 3) | 0;
        let nic_bdf = (3 << 3) | 0;
        assert!(device_matches_scope(igpu_bdf, &scope));
        assert!(!device_matches_scope(nic_bdf, &scope));
    }
}
