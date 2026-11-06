//! `/bin/vmxnet3` PCI identity table (`reference-nic-cell-contract.md`).
//!
//! * `15AD:07B0` — VMware vmxnet3, the paravirtual NIC VMware recommends and
//!   QEMU's `vmxnet3` model. The only entry, `Driveable`.
//!
//! VMware's older `vmxnet` (`15AD:0720`) is a different register model and is
//! refused like any unknown ID.

/// VMware's PCI vendor ID.
pub const VENDOR_VMWARE: u16 = 0x15AD;

/// Whether the cell may bind an ID.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Admission {
    /// Implemented and validated end to end; queried at discovery.
    Driveable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Identity {
    pub device_id: u16,
    pub admission: Admission,
    pub name: &'static str,
}

/// Every ID this cell knows, all under [`VENDOR_VMWARE`].
pub const IDENTITIES: [Identity; 1] = [Identity {
    device_id: 0x07B0,
    admission: Admission::Driveable,
    name: "vmxnet3",
}];

/// The `Driveable` device IDs, in discovery order (424 queries).
pub const QUERY_DEVICE_IDS: [u16; 1] = [0x07B0];

/// The identity of `(vendor_id, device_id)` when this cell may drive it.
pub fn classify(vendor_id: u16, device_id: u16) -> Option<Identity> {
    if vendor_id != VENDOR_VMWARE {
        return None;
    }
    IDENTITIES
        .iter()
        .find(|identity| identity.device_id == device_id)
        .filter(|identity| identity.admission == Admission::Driveable)
        .copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_vmxnet3_is_driveable_and_queried() {
        assert_eq!(classify(0x15AD, 0x07B0).map(|i| i.name), Some("vmxnet3"));
        // vmxnet v1/v2, PVSCSI, a foreign vendor with the same device ID, and
        // an Intel NIC are all refused.
        for (vendor, device) in [
            (0x15AD, 0x0720),
            (0x15AD, 0x07C0),
            (0x8086, 0x07B0),
            (0x8086, 0x10D3),
        ] {
            assert_eq!(classify(vendor, device), None, "{vendor:04x}:{device:04x}");
        }
        let driveable: Vec<u16> = IDENTITIES
            .iter()
            .filter(|i| i.admission == Admission::Driveable)
            .map(|i| i.device_id)
            .collect();
        assert_eq!(driveable, QUERY_DEVICE_IDS);
    }
}
