//! Host tests for `regs.rs` against a fake register file with device-shaped
//! command results.

use super::*;
use std::cell::RefCell;
use std::collections::BTreeMap;

/// Plain storage plus the command register's query results.
struct Fake {
    regs: RefCell<BTreeMap<usize, u32>>,
    results: BTreeMap<u32, u32>,
    writes: RefCell<Vec<(usize, u32)>>,
}

impl Fake {
    fn new(vrrs: u32, uvrs: u32) -> Self {
        let regs = BTreeMap::from([(VD_VRRS, vrrs), (VD_UVRS, uvrs)]);
        Self {
            regs: RefCell::new(regs),
            results: BTreeMap::new(),
            writes: RefCell::new(Vec::new()),
        }
    }
}

impl RegisterFile for Fake {
    fn rd32(&self, offset: usize) -> ViResult<u32> {
        let value = *self.regs.borrow().get(&offset).unwrap_or(&0);
        if offset == VD_CMD {
            return Ok(*self.results.get(&value).unwrap_or(&u32::MAX));
        }
        Ok(value)
    }

    fn wr32(&self, offset: usize, value: u32) -> ViResult<()> {
        self.writes.borrow_mut().push((offset, value));
        self.regs.borrow_mut().insert(offset, value);
        Ok(())
    }
}

#[test]
fn negotiation_selects_revision_1_or_refuses_by_value() {
    let fake = Fake::new(0b11, 0b1);
    assert_eq!(negotiate(&fake).unwrap(), Ok(()));
    assert_eq!(
        *fake.writes.borrow(),
        [(VD_VRRS, REVISION_1), (VD_UVRS, REVISION_1)]
    );
    assert_eq!(
        negotiate(&Fake::new(0b10, 1)).unwrap(),
        Err(Refusal::Revision(0b10))
    );
    let fake = Fake::new(1, 0);
    assert_eq!(negotiate(&fake).unwrap(), Err(Refusal::Upt(0)));
    assert!(!fake.writes.borrow().iter().any(|&(o, _)| o == VD_UVRS));
    // A BAR that does not decode reads all-ones: refused, not taken as an offer.
    assert_eq!(
        negotiate(&Fake::new(u32::MAX, 1)).unwrap(),
        Err(Refusal::Revision(u32::MAX))
    );
    assert_eq!(
        negotiate(&Fake::new(1, u32::MAX)).unwrap(),
        Err(Refusal::Upt(u32::MAX))
    );
}

#[test]
fn permanent_mac_is_two_little_endian_queries() {
    let mut fake = Fake::new(1, 1);
    fake.results.insert(CMD_GET_PERM_MAC_LO, 0x1200_5452);
    fake.results.insert(CMD_GET_PERM_MAC_HI, 0x5634);
    let mac = permanent_mac(&fake).unwrap();
    assert_eq!(mac, [0x52, 0x54, 0x00, 0x12, 0x34, 0x56]);
    assert!(mac_usable(mac));
    assert!(!mac_usable([0; 6]));
    assert!(!mac_usable([0x01, 0, 0x5e, 0, 0, 1]));
    set_mac(&fake, mac).unwrap();
    let writes = fake.writes.borrow();
    assert_eq!(
        writes[writes.len() - 2..],
        [(VD_MACL, 0x1200_5452), (VD_MACH, 0x5634)]
    );
}

// 4 KiB BARs (QEMU's and VMware's) hold every register the cell touches.
const _: () = assert!(PT_MIN <= 0x1000 && VD_MIN <= 0x1000);

#[test]
fn register_map_matches_the_device_interface() {
    assert_eq!((pt_imask(0), pt_imask(2)), (0x000, 0x010));
    assert_eq!((PT_TXPROD, PT_RXPROD, PT_RXPROD2), (0x600, 0x800, 0xA00));
    assert_eq!((VD_DSL, VD_DSH, VD_CMD, VD_ECR), (0x10, 0x18, 0x20, 0x40));
    assert_eq!(CMD_ACTIVATE_DEV, 0xCAFE_0000);
    assert_eq!(CMD_UPDATE_MAC_FILTERS, 0xCAFE_0004);
    assert_eq!(CMD_GET_LINK, 0xF00D_0002);
}
