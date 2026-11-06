//! Intel 8254x PCI identity table for `/bin/e1000`.
//!
//! IDs come from the Intel PCI/PCI-X Family of Gigabit Ethernet Controllers
//! Software Developer's Manual (8254x SDM) §5.2 "Device Identification":
//! 82540EM = `0x100E`, 82545EM copper = `0x100F`. Both parts share the register
//! model this cell implements (legacy descriptors at `0x2800`/`0x3800`, the
//! 8254x `EERD` layout with the address at bits 15:8 and done at bit 4), so
//! admitting the 82545EM is an identity-table change only (plan phase 08, A2).
//!
//! QEMU models both (`e1000` = 82540EM, `e1000-82545em` = 82545EM), and they are
//! VMware's default "e1000" NIC. Every ID outside this table is a refusal that
//! names vendor:device; there is no class-triple wildcard any more (H-S3-6):
//! the cell names its controllers through `FindPcieDeviceByVendor` (424).

/// Intel's PCI vendor ID.
pub const VENDOR_INTEL: u16 = 0x8086;

/// 82540EM (QEMU `e1000`). Model-validated.
pub const I82540EM: u16 = 0x100E;

/// 82545EM copper (QEMU `e1000-82545em`, VMware "e1000"). Model-validated.
pub const I82545EM_COPPER: u16 = 0x100F;

/// Every device ID this cell hands to `sys_find_pcie_device_by_vendor`, in query
/// order, all under [`VENDOR_INTEL`]. The kernel's Ethernet family gate
/// (`ETHERNET_DRIVER_IDS`) lists the same two IDs with `/bin/e1000` as owner.
pub const QUERY_DEVICE_IDS: [u16; 2] = [I82540EM, I82545EM_COPPER];

// Compile-time check (this cell sets `test = false`): every queried ID
// classifies as a driveable part.
const _: () = {
    let mut i = 0;
    while i < QUERY_DEVICE_IDS.len() {
        assert!(!matches!(
            classify(VENDOR_INTEL, QUERY_DEVICE_IDS[i]),
            E1000Sku::Unsupported
        ));
        i += 1;
    }
    assert!(matches!(
        classify(VENDOR_INTEL, 0x10D3),
        E1000Sku::Unsupported
    ));
    assert!(matches!(
        classify(VENDOR_INTEL, 0x10C9),
        E1000Sku::Unsupported
    ));
    assert!(matches!(
        classify(0x15AD, I82545EM_COPPER),
        E1000Sku::Unsupported
    ));
};

/// The part this cell drives, as decided by the PCI ID.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum E1000Sku {
    /// 82540EM.
    I82540Em,
    /// 82545EM copper.
    I82545Em,
    /// Anything else: the caller must fail closed and name vendor:device.
    Unsupported,
}

/// Classify a `(vendor, device)` pair.
pub const fn classify(vendor_id: u16, device_id: u16) -> E1000Sku {
    if vendor_id != VENDOR_INTEL {
        return E1000Sku::Unsupported;
    }
    match device_id {
        I82540EM => E1000Sku::I82540Em,
        I82545EM_COPPER => E1000Sku::I82545Em,
        _ => E1000Sku::Unsupported,
    }
}

/// Part name for the bind log line.
pub const fn sku_name(sku: E1000Sku) -> &'static str {
    match sku {
        E1000Sku::I82540Em => "82540EM",
        E1000Sku::I82545Em => "82545EM",
        E1000Sku::Unsupported => "unsupported",
    }
}
