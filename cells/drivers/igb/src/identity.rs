//! Intel `igb` PCI identity table — the **datasheet** IDs, kept separate from
//! the **QEMU model** ID.
//!
//! The two lists are deliberately not merged (phase 04a):
//!
//! * [`QEMU_MODEL_ID`] — `8086:10c9`. QEMU's `igb` device emulates an 82576
//!   (`hw/net/igb.c`: `c->device_id = E1000_DEV_ID_82576`); this is the only ID
//!   any gate in this lane can validate end to end.
//! * [`I210_IDS`] / [`I211_IDS`] — the real SKU IDs, read from the datasheet:
//!   Intel Ethernet Controller I210 Datasheet §6.2.5 "Device ID (Word 0x0D)"
//!   gives the I210 copper-only default as `0x1533` and points at §9.3.2 for the
//!   other SKUs; the I211 SKU is `0x1539` (§3.4 iNVM and the I211 proxy-flow
//!   note). The fibre/SerDes/SGMII and flash-less values are that same
//!   datasheet table as transcribed by the upstream `igb` driver
//!   (`drivers/net/ethernet/intel/igb/e1000_hw.h`): `I210_COPPER 0x1533`,
//!   `I210_FIBER 0x1536`, `I210_SERDES 0x1537`, `I210_SGMII 0x1538`,
//!   `I210_COPPER_FLASHLESS 0x157B`, `I210_SERDES_FLASHLESS 0x157C`,
//!   `I211_COPPER 0x1539`.
//!
//! None of the datasheet IDs is validated by the QEMU model (plan `A-01`: the
//! model is 82576-class, not bit-exact i210/i211 — PHY and NVM differ), so they
//! bind with the same register model but without a QEMU gate. Every ID outside
//! this table is a *refusal* that names vendor:device; there is no wildcard.

/// Intel's PCI vendor ID.
pub const VENDOR_INTEL: u16 = 0x8086;

/// QEMU's `igb` model: 82576-class, `8086:10c9`. Model-validated.
pub const QEMU_MODEL_ID: u16 = 0x10C9;

/// i210 SKU device IDs (datasheet §9.3.2 / upstream `igb` ID table).
///
/// **Target list, not a claim** — see [`SUPPORTED_DEVICE_IDS`] for what this cell
/// can actually drive today.
pub const I210_IDS: [u16; 6] = [0x1533, 0x1536, 0x1537, 0x1538, 0x157B, 0x157C];

/// i211 SKU device ID (datasheet §3.4 / I211 proxy flow). Target only: the i211
/// reads its MAC from iNVM, which this cell does not implement yet.
pub const I211_IDS: [u16; 1] = [0x1539];

/// The device IDs this cell actually claims: the ones whose NVM access and media
/// path it implements.
///
/// * `0x10C9` — QEMU's 82576 model, the only ID validated in this lane.
/// * `0x1533` — i210 copper **with external flash**, so the EERD/Shadow-RAM read
///   implemented here returns the MAC.
///
/// Deliberately absent, with the prerequisite each needs (recorded in the phase
/// evidence so a future claim has to implement it rather than widen the table):
/// `0x157B`/`0x157C` (i210 flashless) and `0x1539` (i211) need the **iNVM** read
/// path; `0x1536`/`0x1537`/`0x1538` (fibre/SerDes/SGMII) need **media-specific
/// link setup** rather than the copper BMCR restart. Claiming any of them without
/// that work would register a device this cell cannot drive.
pub const SUPPORTED_DEVICE_IDS: [u16; 2] = [QEMU_MODEL_ID, 0x1533];

/// Every device ID this cell hands to `sys_find_pcie_device_by_vendor`, in query
/// order, all under [`VENDOR_INTEL`].
///
/// Naming the controller exactly is what removes the sibling race: `/bin/e1000`
/// can never hold this device, so there is no decline-and-retry window.
pub const QUERY_DEVICE_IDS: [u16; 2] = SUPPORTED_DEVICE_IDS;

// Compile-time check (this cell sets `test = false`, so a `#[cfg(test)]` module
// would never be compiled): the query list is exactly the supported set, every
// entry classifies as a driveable SKU, and the unimplemented SKUs stay out until
// their prerequisite lands.
const _: () = {
    assert!(QUERY_DEVICE_IDS[0] == QEMU_MODEL_ID);
    assert!(QUERY_DEVICE_IDS[1] == 0x1533);
    assert!(QUERY_DEVICE_IDS.len() == SUPPORTED_DEVICE_IDS.len());
    let mut i = 0;
    while i < QUERY_DEVICE_IDS.len() {
        assert!(!matches!(
            classify(VENDOR_INTEL, QUERY_DEVICE_IDS[i]),
            IgbSku::Unsupported
        ));
        i += 1;
    }
};

/// The register model this cell implements, as decided by the PCI ID.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IgbSku {
    /// 82576 — the QEMU model, the only ID validated in this lane.
    I82576,
    /// A real i210 SKU; same register model, not model-validated.
    I210,
    /// The i211 SKU; same register model, not model-validated.
    I211,
    /// Anything else: the caller must fail closed and name vendor:device.
    Unsupported,
}

/// Classify a `(vendor, device)` pair.
///
/// `const fn` so the same table can be used by tests and by the bind path
/// without allocating; `while` loops are the only construct available here.
pub const fn classify(vendor_id: u16, device_id: u16) -> IgbSku {
    if vendor_id != VENDOR_INTEL {
        return IgbSku::Unsupported;
    }
    if device_id == QEMU_MODEL_ID {
        return IgbSku::I82576;
    }
    let mut i = 0;
    while i < I210_IDS.len() {
        if device_id == I210_IDS[i] {
            return IgbSku::I210;
        }
        i += 1;
    }
    let mut i = 0;
    while i < I211_IDS.len() {
        if device_id == I211_IDS[i] {
            return IgbSku::I211;
        }
        i += 1;
    }
    IgbSku::Unsupported
}

/// Human-readable family name for the bind log line.
pub const fn sku_name(sku: IgbSku) -> &'static str {
    match sku {
        IgbSku::I82576 => "82576 (QEMU model)",
        IgbSku::I210 => "i210 (datasheet)",
        IgbSku::I211 => "i211 (datasheet)",
        IgbSku::Unsupported => "unsupported",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qemu_model_is_recognised() {
        assert_eq!(classify(VENDOR_INTEL, QEMU_MODEL_ID), IgbSku::I82576);
    }

    #[test]
    fn datasheet_skus_are_recognised() {
        assert_eq!(classify(VENDOR_INTEL, 0x1533), IgbSku::I210);
        assert_eq!(classify(VENDOR_INTEL, 0x1536), IgbSku::I210);
        assert_eq!(classify(VENDOR_INTEL, 0x1537), IgbSku::I210);
        assert_eq!(classify(VENDOR_INTEL, 0x1538), IgbSku::I210);
        assert_eq!(classify(VENDOR_INTEL, 0x157B), IgbSku::I210);
        assert_eq!(classify(VENDOR_INTEL, 0x157C), IgbSku::I210);
        assert_eq!(classify(VENDOR_INTEL, 0x1539), IgbSku::I211);
    }

    #[test]
    fn other_families_fail_closed() {
        // The e1000 the sibling cell owns must never classify as igb.
        assert_eq!(classify(VENDOR_INTEL, 0x100E), IgbSku::Unsupported);
        // e1000e/I219 stays a recorded follow-up, not a silent accept.
        assert_eq!(classify(VENDOR_INTEL, 0x10D3), IgbSku::Unsupported);
        assert_eq!(classify(VENDOR_INTEL, 0x15A1), IgbSku::Unsupported);
        assert_eq!(classify(0x1AF4, QEMU_MODEL_ID), IgbSku::Unsupported);
    }
}
