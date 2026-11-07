//! RMRR (Reserved Memory Region Reporting) support (VT-d spec §8.4).

use super::super::iommu_pt::VtdSlpt;
use crate::acpi::{DeviceScope, RmrrEntry};
use alloc::vec::Vec;

pub fn device_matches_scope(bdf: u16, scope: &DeviceScope) -> bool {
    let bus = ((bdf >> 8) & 0xFF) as u8;
    let dev = ((bdf >> 3) & 0x1F) as u8;
    let func = (bdf & 0x07) as u8;

    if scope.path_len == 0 {
        return false;
    }
    if scope.kind == 1 {
        // PCI Endpoint Device: bus must match start_bus, path[0] == [dev, func]
        if bus == scope.start_bus && scope.path_len == 1 {
            let [p_dev, p_func] = scope.path[0];
            return p_dev == dev && p_func == func;
        }
    } else if scope.kind == 2 {
        // PCI-to-PCI Bridge: any device under the bridge's subtree
        // Single bridge: device is on downstream bus
        if bus >= scope.start_bus {
            return true;
        }
    }
    false
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
    use crate::acpi::SCOPE_PATH_MAX;

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
